// SPDX-License-Identifier: MPL-2.0

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::transport::{HidTransport, REPORT_SIZE, Transport};
use crate::{Error, Result};

pub(crate) const DATA_OFFSET: usize = 7;
const CRC_OFFSET: usize = 62;
const MAX_PAYLOAD: usize = CRC_OFFSET - DATA_OFFSET;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING: usize = 32;
const OPEN_SESSION: Command = Command::new(0x01, 0x01);
const CLOSE_SESSION: Command = Command::new(0x01, 0x02);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct Command {
    pub(crate) group: u8,
    pub(crate) command: u8,
}

impl Command {
    pub(crate) const fn new(group: u8, command: u8) -> Self {
        Self { group, command }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Frame {
    pub(crate) status: u8,
    pub(crate) sequence: u8,
    pub(crate) command: Command,
    pub(crate) payload: Vec<u8>,
}

pub(crate) fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub(crate) fn build_frame(
    session: u8,
    sequence: u8,
    command: Command,
    payload: &[u8],
) -> Result<[u8; REPORT_SIZE]> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge {
            actual: payload.len(),
            maximum: MAX_PAYLOAD,
        });
    }
    let mut report = [0_u8; REPORT_SIZE];
    report[0] = u8::try_from(6 + payload.len()).expect("QLink payload bound fits in u8");
    report[2] = session;
    report[4] = sequence;
    report[5] = command.group;
    report[6] = command.command;
    report[DATA_OFFSET..DATA_OFFSET + payload.len()].copy_from_slice(payload);
    let crc = crc16_modbus(&report[..CRC_OFFSET]).to_le_bytes();
    report[CRC_OFFSET..].copy_from_slice(&crc);
    Ok(report)
}

pub(crate) fn parse_frame(report: &[u8; REPORT_SIZE]) -> Result<Frame> {
    if !(6..CRC_OFFSET as u8).contains(&report[0]) {
        return Err(Error::InvalidFrameLength(report[0]));
    }
    validate_crc(report)?;
    let payload_end = usize::from(report[0]) + 1;
    Ok(Frame {
        status: report[3],
        sequence: report[4],
        command: Command::new(report[5], report[6]),
        payload: report[DATA_OFFSET..payload_end].to_vec(),
    })
}

fn validate_crc(report: &[u8; REPORT_SIZE]) -> Result<()> {
    let received = u16::from_le_bytes([report[CRC_OFFSET], report[CRC_OFFSET + 1]]);
    let calculated = crc16_modbus(&report[..CRC_OFFSET]);
    if received != calculated {
        return Err(Error::CrcMismatch {
            received,
            calculated,
        });
    }
    Ok(())
}

fn parse_continuation(
    report: &[u8; REPORT_SIZE],
    expected_index: u8,
    expected_session: u8,
) -> Result<Vec<u8>> {
    if !(3..CRC_OFFSET as u8).contains(&report[0]) {
        return Err(Error::InvalidContinuationLength(report[0]));
    }
    validate_crc(report)?;
    let actual_index = report[1] & 0x7f;
    if actual_index != expected_index {
        return Err(Error::WrongContinuationIndex {
            actual: actual_index,
            expected: expected_index,
        });
    }
    if report[2] != expected_session {
        return Err(Error::WrongContinuationSession {
            actual: report[2],
            expected: expected_session,
        });
    }
    Ok(report[3..=usize::from(report[0])].to_vec())
}

pub(crate) struct QLink<T: Transport> {
    transport: T,
    timeout: Duration,
    sequence: u8,
    session: u8,
    pending: VecDeque<Frame>,
}

impl QLink<HidTransport> {
    pub(crate) fn open() -> Result<Self> {
        Self::with_transport(HidTransport::open()?)
    }
}

impl<T: Transport> QLink<T> {
    fn with_transport(transport: T) -> Result<Self> {
        let mut link = Self {
            transport,
            timeout: DEFAULT_TIMEOUT,
            sequence: 0x30,
            session: 0,
            pending: VecDeque::new(),
        };
        link.drain()?;
        link.open_session()?;
        Ok(link)
    }

    fn next_sequence(&mut self) -> u8 {
        self.sequence = self.sequence.wrapping_add(1);
        if self.sequence == 0 {
            self.sequence = 1;
        }
        self.sequence
    }

    fn drain(&mut self) -> Result<()> {
        while self.transport.read_report(Duration::ZERO)?.is_some() {}
        Ok(())
    }

    fn open_session(&mut self) -> Result<()> {
        let mut random = [0_u8; 4];
        getrandom::fill(&mut random).map_err(|error| Error::Random(error.to_string()))?;
        let nonce = (u32::from_le_bytes(random) % 90_000 + 10_000).to_le_bytes();
        let mut payload = nonce.to_vec();
        payload.push(2);
        let response = self.exchange(OPEN_SESSION, &payload, Some(0))?;
        if response.len() < 7 {
            return Err(Error::ShortSessionResponse(response.len()));
        }
        if response[..4] != nonce {
            return Err(Error::SessionNonceMismatch);
        }
        if response[4] == 0 {
            return Err(Error::InvalidSession);
        }
        self.session = response[4];
        Ok(())
    }

    pub(crate) fn request(&mut self, command: Command, payload: &[u8]) -> Result<Vec<u8>> {
        self.exchange(command, payload, None)
    }

    pub(crate) fn read_continuation(&mut self, expected_index: u8) -> Result<Vec<u8>> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::ContinuationTimeout(expected_index));
            }
            let Some(report) = self.transport.read_report(deadline - now)? else {
                return Err(Error::ContinuationTimeout(expected_index));
            };
            if report[1] != 0 {
                return parse_continuation(&report, expected_index, self.session);
            }
            let frame = parse_frame(&report)?;
            if self.pending.len() == MAX_PENDING {
                return Err(Error::PendingOverflow);
            }
            self.pending.push_back(frame);
        }
    }

    pub(crate) fn read_notification<F>(
        &mut self,
        command: Command,
        timeout: Duration,
        predicate: F,
    ) -> Result<Option<Frame>>
    where
        F: Fn(&Frame) -> bool,
    {
        if let Some(index) = self
            .pending
            .iter()
            .position(|frame| frame.command == command && predicate(frame))
        {
            return Ok(self.pending.remove(index));
        }
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
            if frame.command == command && predicate(&frame) {
                return Ok(Some(frame));
            }
            if self.pending.len() == MAX_PENDING {
                return Err(Error::PendingOverflow);
            }
            self.pending.push_back(frame);
        }
    }

    fn exchange(
        &mut self,
        command: Command,
        payload: &[u8],
        session: Option<u8>,
    ) -> Result<Vec<u8>> {
        let sequence = self.next_sequence();
        let report = build_frame(session.unwrap_or(self.session), sequence, command, payload)?;
        self.transport.write_report(&report)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout {
                    group: command.group,
                    command: command.command,
                    sequence,
                });
            }
            let Some(report) = self.transport.read_report(deadline - now)? else {
                return Err(Error::Timeout {
                    group: command.group,
                    command: command.command,
                    sequence,
                });
            };
            let frame = parse_frame(&report)?;
            if frame.command != command || frame.sequence != sequence {
                if self.pending.len() == MAX_PENDING {
                    return Err(Error::PendingOverflow);
                }
                self.pending.push_back(frame);
                continue;
            }
            if frame.status != 0 {
                return Err(Error::DeviceStatus {
                    group: command.group,
                    command: command.command,
                    status: frame.status,
                });
            }
            return Ok(frame.payload);
        }
    }

    pub(crate) fn close(&mut self) -> Result<()> {
        if self.session == 0 {
            return Ok(());
        }
        self.exchange(CLOSE_SESSION, &[], None)?;
        self.session = 0;
        Ok(())
    }

    fn close_without_waiting(&mut self) {
        if self.session == 0 {
            return;
        }
        let sequence = self.next_sequence();
        let report = build_frame(self.session, sequence, CLOSE_SESSION, &[])
            .expect("an empty close-session payload always fits");
        self.session = 0;
        let _ = self.transport.write_report(&report);
    }
}

impl<T: Transport> Drop for QLink<T> {
    fn drop(&mut self) {
        self.close_without_waiting();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    struct RecordingTransport {
        writes: Rc<RefCell<Vec<[u8; REPORT_SIZE]>>>,
    }

    impl Transport for RecordingTransport {
        fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> Result<()> {
            self.writes.borrow_mut().push(*report);
            Ok(())
        }

        fn read_report(&mut self, _timeout: Duration) -> Result<Option<[u8; REPORT_SIZE]>> {
            Ok(None)
        }
    }

    #[test]
    fn crc_matches_modbus_check_value() {
        assert_eq!(crc16_modbus(b"123456789"), 0x4b37);
    }

    #[test]
    fn frame_round_trip() {
        let command = Command::new(0x20, 0x03);
        let report = build_frame(7, 49, command, &[0x6d, 0, 1, 2, 3, 4, 9]).unwrap();
        let parsed = parse_frame(&report).unwrap();
        assert_eq!(parsed.status, 0);
        assert_eq!(parsed.sequence, 49);
        assert_eq!(parsed.command, command);
        assert_eq!(parsed.payload, [0x6d, 0, 1, 2, 3, 4, 9]);
    }

    #[test]
    fn continuation_uses_observed_three_byte_header() {
        let mut report = [0_u8; REPORT_SIZE];
        report[0] = 0x25;
        report[1] = 1;
        report[2] = 7;
        let payload = [0, 2, 5, 0x6f, 0, 7, 1];
        report[3..3 + payload.len()].copy_from_slice(&payload);
        let crc = crc16_modbus(&report[..CRC_OFFSET]).to_le_bytes();
        report[CRC_OFFSET..].copy_from_slice(&crc);
        let parsed = parse_continuation(&report, 1, 7).unwrap();
        assert_eq!(&parsed[..payload.len()], payload);
        assert_eq!(parsed.len(), 35);
    }

    #[test]
    fn non_final_continuation_carries_more_fragments_bit() {
        let mut report = [0_u8; REPORT_SIZE];
        report[0] = 5;
        report[1] = 0x81;
        report[2] = 7;
        report[3..=5].copy_from_slice(&[0xaa, 0xbb, 0xcc]);
        let crc = crc16_modbus(&report[..CRC_OFFSET]).to_le_bytes();
        report[CRC_OFFSET..].copy_from_slice(&crc);
        assert_eq!(
            parse_continuation(&report, 1, 7).unwrap(),
            [0xaa, 0xbb, 0xcc]
        );
    }

    #[test]
    fn dropping_an_open_link_sends_close_without_waiting() {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let link = QLink {
            transport: RecordingTransport {
                writes: Rc::clone(&writes),
            },
            timeout: DEFAULT_TIMEOUT,
            sequence: 0x30,
            session: 7,
            pending: VecDeque::new(),
        };
        drop(link);
        let writes = writes.borrow();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0][2], 7);
        assert_eq!(writes[0][5], CLOSE_SESSION.group);
        assert_eq!(writes[0][6], CLOSE_SESSION.command);
    }
}
