// SPDX-License-Identifier: MPL-2.0

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::display_keys::DisplayKey;
use crate::qlink::{Command, parse_frame};
use crate::transport::{HidTransport, Transport};
use crate::{Error, Result};

const KEY_EVENT: Command = Command::new(0x11, 0x02);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub struct KeyEvent {
    pub key: u8,
    pub pressed: bool,
    pub assigned_action: [u8; 2],
}

pub struct EventStream {
    transport: HidTransport,
}

impl EventStream {
    pub fn open() -> Result<Self> {
        Ok(Self {
            transport: HidTransport::open()?,
        })
    }

    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<KeyEvent>> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let Some(report) = self.transport.read_report(deadline - now)? else {
                return Ok(None);
            };
            let frame = parse_frame(&report)?;
            if frame.command != KEY_EVENT {
                continue;
            }
            return Ok(Some(parse_key_event(&frame.payload)?));
        }
    }
}

fn parse_key_event(payload: &[u8]) -> Result<KeyEvent> {
    if payload.len() < 5 {
        return Err(Error::ShortKeyEvent(payload.len()));
    }
    let key = DisplayKey::from_device_id(payload[0]).ok_or(Error::UnknownEventKey(payload[0]))?;
    Ok(KeyEvent {
        key: key.number(),
        pressed: payload[2] == 1,
        assigned_action: [payload[3], payload[4]],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_display_key_press() {
        assert_eq!(
            parse_key_event(&[0x6f, 0, 1, 0x12, 0x34]).unwrap(),
            KeyEvent {
                key: 3,
                pressed: true,
                assigned_action: [0x12, 0x34],
            }
        );
        assert!(parse_key_event(&[0x20, 0, 1, 0, 0]).is_err());
    }
}
