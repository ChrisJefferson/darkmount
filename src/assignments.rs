// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;

use crate::device::Keyboard;
use crate::display_keys::DisplayKey;
use crate::qlink::Command;
use crate::{Error, Result, WriteOutcome};

const READ_ASSIGNMENTS: Command = Command::new(0x11, 0x01);
const WRITE_ASSIGNMENT: Command = Command::new(0x11, 0x02);
const RESTORE_DEFAULT: Command = Command::new(0x11, 0x03);
const MAX_TEXT_BYTES: usize = 51;
const F13_KEY_CODE: u8 = 0x68;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const LEFT_CONTROL: Self = Self(1 << 0);
    pub const LEFT_SHIFT: Self = Self(1 << 1);
    pub const LEFT_ALT: Self = Self(1 << 2);
    pub const LEFT_GUI: Self = Self(1 << 3);
    pub const RIGHT_CONTROL: Self = Self(1 << 4);
    pub const RIGHT_SHIFT: Self = Self(1 << 5);
    pub const RIGHT_ALT: Self = Self(1 << 6);
    pub const RIGHT_GUI: Self = Self(1 << 7);

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssignmentAction {
    Disabled,
    StandardKey { modifiers: Modifiers, key_code: u8 },
    NextEffect,
    Subtype { action_type: u8, subtype: u8 },
    Application { value: String },
    Website { value: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Assignment {
    pub key_id: u16,
    pub display_key: Option<u8>,
    pub action: AssignmentAction,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub struct DisplayFunctionKeyOutcome {
    pub display_key: u8,
    pub function_key: u8,
    pub hid_key_code: u8,
    pub outcome: WriteOutcome,
}

impl Keyboard {
    pub fn read_assignments(&mut self) -> Result<Vec<Assignment>> {
        self.require_control_support()?;
        let mut payload = self.request(READ_ASSIGNMENTS, &[0, 0])?;
        let mut continuation_index = 1_u8;
        while !assignment_response_complete(&payload)? {
            payload.extend_from_slice(&self.read_continuation(continuation_index)?);
            continuation_index = continuation_index
                .checked_add(1)
                .expect("assignment response uses fewer than 256 fragments");
        }
        parse_assignments(&payload)
    }

    pub fn write_assignment(
        &mut self,
        key_id: u16,
        action: &AssignmentAction,
    ) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if current_action(&self.read_assignments()?, key_id) == Some(action) {
            return Ok(WriteOutcome::Unchanged);
        }
        let mut payload = key_id.to_le_bytes().to_vec();
        action.encode(key_id, &mut payload)?;
        self.request(WRITE_ASSIGNMENT, &payload)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        if current_action(&self.read_assignments()?, key_id) != Some(action) {
            return Err(Error::VerificationFailed {
                target: "key assignment",
            });
        }
        Ok(WriteOutcome::Written)
    }

    pub fn restore_default_assignment(
        &mut self,
        key_id: u16,
        expected: Option<&AssignmentAction>,
    ) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if current_action(&self.read_assignments()?, key_id) == expected {
            return Ok(WriteOutcome::Unchanged);
        }
        self.request(RESTORE_DEFAULT, &key_id.to_le_bytes())?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        if current_action(&self.read_assignments()?, key_id) != expected {
            return Err(Error::VerificationFailed {
                target: "default key assignment",
            });
        }
        Ok(WriteOutcome::Written)
    }

    pub fn assign_display_function_keys(&mut self) -> Result<Vec<DisplayFunctionKeyOutcome>> {
        let mut outcomes = Vec::with_capacity(8);
        for display_key in 1..=8 {
            let function_key = display_key + 12;
            let hid_key_code = F13_KEY_CODE + display_key - 1;
            let action = AssignmentAction::StandardKey {
                modifiers: Modifiers::NONE,
                key_code: hid_key_code,
            };
            let outcome = self
                .write_assignment(0x6c + u16::from(display_key), &action)
                .map_err(|source| Error::DisplayFunctionKeyPreset {
                    display_key,
                    function_key,
                    source: Box::new(source),
                })?;
            outcomes.push(DisplayFunctionKeyOutcome {
                display_key,
                function_key,
                hid_key_code,
                outcome,
            });
        }
        Ok(outcomes)
    }
}

impl AssignmentAction {
    fn encode(&self, key_id: u16, payload: &mut Vec<u8>) -> Result<()> {
        if matches!(
            self,
            Self::NextEffect | Self::Application { .. } | Self::Website { .. }
        ) && display_key_number(key_id).is_none()
        {
            return Err(Error::AssignmentRequiresDisplayKey(key_id));
        }
        match self {
            Self::Disabled => payload.push(0),
            Self::StandardKey {
                modifiers,
                key_code,
            } => {
                payload.extend_from_slice(&[1, modifiers.bits(), *key_code]);
                if display_key_number(key_id).is_some() {
                    payload.push(0);
                }
            }
            Self::NextEffect => payload.extend_from_slice(&[9, 2]),
            Self::Application { value } => encode_text_action(5, value, payload)?,
            Self::Website { value } => encode_text_action(6, value, payload)?,
            Self::Subtype { .. } => return Err(Error::UnsupportedAssignmentWrite),
        }
        Ok(())
    }
}

fn encode_text_action(action_type: u8, value: &str, payload: &mut Vec<u8>) -> Result<()> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(Error::InvalidAssignmentTextLength(value.len()));
    }
    payload.extend_from_slice(&[action_type, value.len() as u8]);
    payload.extend_from_slice(value.as_bytes());
    Ok(())
}

fn current_action(assignments: &[Assignment], key_id: u16) -> Option<&AssignmentAction> {
    assignments
        .iter()
        .find(|assignment| assignment.key_id == key_id)
        .map(|assignment| &assignment.action)
}

fn display_key_number(key_id: u16) -> Option<u8> {
    u8::try_from(key_id)
        .ok()
        .and_then(DisplayKey::from_device_id)
        .map(DisplayKey::number)
}

fn assignment_response_complete(payload: &[u8]) -> Result<bool> {
    if payload.len() < 2 {
        return Err(Error::ShortAssignmentResponse(payload.len()));
    }
    let count = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
    let mut offset = 2;
    for index in 0..count {
        let Some(size) = assignment_entry_size(&payload[offset..], index)? else {
            return Ok(false);
        };
        offset += size;
    }
    if payload.len() != offset {
        return Err(Error::InvalidAssignmentResponseLength {
            actual: payload.len(),
            expected: offset,
        });
    }
    Ok(true)
}

fn assignment_entry_size(entry: &[u8], index: usize) -> Result<Option<usize>> {
    if entry.len() < 3 {
        return Ok(None);
    }
    let size = match entry[2] {
        0 => 3,
        1 => {
            let key_id = u16::from_le_bytes([entry[0], entry[1]]);
            if display_key_number(key_id).is_some() {
                6
            } else {
                5
            }
        }
        2 | 7 | 9 => 4,
        5 | 6 => {
            if entry.len() < 4 {
                return Ok(None);
            }
            4 + usize::from(entry[3])
        }
        action_type => {
            return Err(Error::UnknownAssignmentActionType { index, action_type });
        }
    };
    Ok((entry.len() >= size).then_some(size))
}

fn parse_assignments(payload: &[u8]) -> Result<Vec<Assignment>> {
    if !assignment_response_complete(payload)? {
        return Err(Error::IncompleteAssignmentResponse(payload.len()));
    }
    let count = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
    let mut assignments = Vec::with_capacity(count);
    let mut offset = 2;
    for index in 0..count {
        let size = assignment_entry_size(&payload[offset..], index)?
            .expect("complete response contains every declared entry");
        let entry = &payload[offset..offset + size];
        let key_id = u16::from_le_bytes([entry[0], entry[1]]);
        if entry[2] == 1 && display_key_number(key_id).is_some() && entry[5] != 0 {
            return Err(Error::UnsupportedStandardKeyParameter(entry[5]));
        }
        let action = match entry[2] {
            0 => AssignmentAction::Disabled,
            1 => AssignmentAction::StandardKey {
                modifiers: Modifiers::from_bits(entry[3]),
                key_code: entry[4],
            },
            9 if entry[3] == 2 => AssignmentAction::NextEffect,
            action_type @ (2 | 7 | 9) => AssignmentAction::Subtype {
                action_type,
                subtype: entry[3],
            },
            action_type @ (5 | 6) => {
                let value = String::from_utf8(entry[4..].to_vec())
                    .map_err(|_| Error::InvalidAssignmentText { index, action_type })?;
                if action_type == 5 {
                    AssignmentAction::Application { value }
                } else {
                    AssignmentAction::Website { value }
                }
            }
            _ => unreachable!("entry size rejected unknown action type"),
        };
        let display_key = display_key_number(key_id);
        assignments.push(Assignment {
            key_id,
            display_key,
            action,
        });
        offset += size;
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_twenty_two() -> Vec<u8> {
        vec![
            0x16, 0x00, 0x42, 0x80, 0x09, 0x05, 0x41, 0x80, 0x09, 0x04, 0x3e, 0x80, 0x09, 0x03,
            0x45, 0x80, 0x09, 0x02, 0x43, 0x80, 0x02, 0x01, 0x44, 0x80, 0x02, 0x02, 0x3f, 0x80,
            0x02, 0x03, 0x40, 0x80, 0x02, 0x05, 0x3c, 0x80, 0x02, 0x07, 0x3d, 0x80, 0x02, 0x06,
            0x75, 0x00, 0x02, 0x03, 0x76, 0x00, 0x02, 0x07, 0x77, 0x00, 0x02, 0x06, 0x78, 0x00,
            0x02, 0x05, 0x6f, 0x00, 0x07, 0x01, 0x71, 0x00, 0x07, 0x08, 0x72, 0x00, 0x07, 0x03,
            0x73, 0x00, 0x07, 0x04, 0x74, 0x00, 0x07, 0x06, 0x6e, 0x00, 0x07, 0x14, 0x70, 0x00,
            0x07, 0x15, 0x6d, 0x00, 0x09, 0x02,
        ]
    }

    #[test]
    fn reconnect_capture_decodes_all_assignments() {
        let assignments = parse_assignments(&captured_twenty_two()).unwrap();
        assert_eq!(assignments.len(), 22);
        let key_one = assignments
            .iter()
            .find(|assignment| assignment.display_key == Some(1))
            .unwrap();
        assert_eq!(key_one.action, AssignmentAction::NextEffect);
    }

    #[test]
    fn response_count_controls_the_number_of_entries() {
        let assignments = parse_assignments(&[1, 0, 0x6d, 0, 0]).unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].display_key, Some(1));
        assert_eq!(assignments[0].action, AssignmentAction::Disabled);
    }

    #[test]
    fn captured_f12_remap_is_a_variable_length_entry() {
        let mut payload = captured_twenty_two();
        payload[0] = 23;
        payload.extend_from_slice(&[0x63, 0x00, 0x01, 0x01, 0x04]);
        let assignments = parse_assignments(&payload).unwrap();
        assert_eq!(assignments.len(), 23);
        assert_eq!(
            assignments.last().unwrap(),
            &Assignment {
                key_id: 0x63,
                display_key: None,
                action: AssignmentAction::StandardKey {
                    modifiers: Modifiers::LEFT_CONTROL,
                    key_code: 4,
                },
            }
        );
    }

    #[test]
    fn incomplete_variable_entry_needs_a_continuation() {
        let mut payload = captured_twenty_two();
        payload[0] = 23;
        payload.extend_from_slice(&[0x63, 0x00, 0x01, 0x01]);
        assert!(!assignment_response_complete(&payload).unwrap());
    }

    #[test]
    fn writes_match_captured_physical_and_display_key_packets() {
        let mut physical = 0x63_u16.to_le_bytes().to_vec();
        AssignmentAction::StandardKey {
            modifiers: Modifiers::LEFT_CONTROL,
            key_code: 0x04,
        }
        .encode(0x63, &mut physical)
        .unwrap();
        assert_eq!(physical, [0x63, 0, 1, 1, 4]);

        let mut display = 0x6d_u16.to_le_bytes().to_vec();
        AssignmentAction::StandardKey {
            modifiers: Modifiers::LEFT_SHIFT,
            key_code: 0x05,
        }
        .encode(0x6d, &mut display)
        .unwrap();
        assert_eq!(display, [0x6d, 0, 1, 2, 5, 0]);
    }

    #[test]
    fn display_actions_match_captured_packets() {
        let mut application = 0x6d_u16.to_le_bytes().to_vec();
        AssignmentAction::Application {
            value: "file:///C:/Users/caj/Desktop/Firefox.exe".to_owned(),
        }
        .encode(0x6d, &mut application)
        .unwrap();
        assert_eq!(&application[..4], &[0x6d, 0, 5, 40]);

        let mut website = 0x6d_u16.to_le_bytes().to_vec();
        AssignmentAction::Website {
            value: "https://www.bequiet.com/en".to_owned(),
        }
        .encode(0x6d, &mut website)
        .unwrap();
        assert_eq!(&website[..4], &[0x6d, 0, 6, 26]);

        let mut media = 0x6d_u16.to_le_bytes().to_vec();
        AssignmentAction::NextEffect
            .encode(0x6d, &mut media)
            .unwrap();
        assert_eq!(media, [0x6d, 0, 9, 2]);
    }

    #[test]
    fn variable_display_actions_round_trip_through_the_reader() {
        let standard = parse_assignments(&[1, 0, 0x6d, 0, 1, 2, 5, 0]).unwrap();
        assert_eq!(
            standard[0].action,
            AssignmentAction::StandardKey {
                modifiers: Modifiers::LEFT_SHIFT,
                key_code: 5,
            }
        );

        let website = parse_assignments(&[1, 0, 0x6d, 0, 6, 3, b'a', b'b', b'c']).unwrap();
        assert_eq!(
            website[0].action,
            AssignmentAction::Website {
                value: "abc".to_owned(),
            }
        );
    }

    #[test]
    fn display_function_key_codes_are_f13_through_f20() {
        for display_key in 1..=8 {
            assert_eq!(F13_KEY_CODE + display_key - 1, 0x67 + display_key);
            assert_eq!(display_key + 12, 12 + display_key);
        }
        assert_eq!(F13_KEY_CODE, 0x68);
        assert_eq!(F13_KEY_CODE + 7, 0x6f);
    }
}
