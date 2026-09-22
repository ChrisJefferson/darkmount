// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;

use crate::device::Keyboard;
use crate::qlink::Command;
use crate::{Error, Result, WriteOutcome};

const READ_SETTINGS: Command = Command::new(0x21, 0x02);
const WRITE_SETTINGS: Command = Command::new(0x21, 0x03);
const SET_TIME: Command = Command::new(0x21, 0x05);
const READ_IMAGE: Command = Command::new(0x21, 0x06);
const WRITE_IMAGE: Command = Command::new(0x21, 0x07);
const HEADER_SIZE: usize = 9;
const READ_CHUNK: usize = 54;
const WRITE_CHUNK: usize = 49;
pub const IMAGE_WIDTH: u16 = 320;
pub const IMAGE_HEIGHT: u16 = 240;
const IMAGE_FORMAT: u8 = 1;
pub const PIXEL_BYTES: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize * 2;
const STORED_IMAGE_BYTES: usize = PIXEL_BYTES + HEADER_SIZE;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockFormat {
    TwelveHour,
    TwentyFourHour,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleDisplay {
    Clock,
    Image,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct DockSettings {
    pub menu_colour: [u8; 3],
    pub clock_format: ClockFormat,
    pub idle_display: IdleDisplay,
    pub idle_seconds: u16,
    pub off_seconds: u16,
}

impl DockSettings {
    fn parse(payload: &[u8]) -> Result<Self> {
        if payload.len() < 9 {
            return Err(Error::ShortDockSettings(payload.len()));
        }
        let clock_format = match payload[3] {
            0 => ClockFormat::TwelveHour,
            1 => ClockFormat::TwentyFourHour,
            value => return Err(Error::InvalidClockFormat(value)),
        };
        let idle_display = match payload[4] {
            1 => IdleDisplay::Clock,
            2 => IdleDisplay::Image,
            value => return Err(Error::InvalidIdleDisplay(value)),
        };
        Ok(Self {
            menu_colour: [payload[0], payload[1], payload[2]],
            clock_format,
            idle_display,
            idle_seconds: u16::from_le_bytes([payload[5], payload[6]]),
            off_seconds: u16::from_le_bytes([payload[7], payload[8]]),
        })
    }

    fn encode(&self) -> [u8; 9] {
        let clock_format = match self.clock_format {
            ClockFormat::TwelveHour => 0,
            ClockFormat::TwentyFourHour => 1,
        };
        let idle_display = match self.idle_display {
            IdleDisplay::Clock => 1,
            IdleDisplay::Image => 2,
        };
        let idle = self.idle_seconds.to_le_bytes();
        let off = self.off_seconds.to_le_bytes();
        [
            self.menu_colour[0],
            self.menu_colour[1],
            self.menu_colour[2],
            clock_format,
            idle_display,
            idle[0],
            idle[1],
            off[0],
            off[1],
        ]
    }
}

impl Keyboard {
    pub fn read_dock_settings(&mut self) -> Result<DockSettings> {
        self.require_control_support()?;
        DockSettings::parse(&self.request(READ_SETTINGS, &[])?)
    }

    pub fn write_dock_settings(&mut self, settings: &DockSettings) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if self.read_dock_settings()? == *settings {
            return Ok(WriteOutcome::Unchanged);
        }
        self.request(WRITE_SETTINGS, &settings.encode())?;
        if self.read_dock_settings()? != *settings {
            return Err(Error::VerificationFailed {
                target: "dock settings",
            });
        }
        Ok(WriteOutcome::Written)
    }

    pub fn set_dock_local_timestamp(&mut self, timestamp: u32) -> Result<bool> {
        self.require_control_support()?;
        self.request(SET_TIME, &timestamp.to_le_bytes())?;
        let confirmation =
            self.read_notification(WRITE_SETTINGS, std::time::Duration::from_secs(2), |frame| {
                frame.sequence == 0 && frame.payload.first() == Some(&4)
            })?;
        let Some(frame) = confirmation else {
            return Ok(false);
        };
        if frame.payload.len() < 5 {
            return Ok(false);
        }
        Ok(
            u32::from_le_bytes(frame.payload[1..5].try_into().expect("four-byte slice"))
                == timestamp,
        )
    }

    pub fn read_dock_image(&mut self) -> Result<Vec<u8>> {
        self.require_control_support()?;
        let header = self.read_dock_image_chunk(0, HEADER_SIZE)?;
        if header.len() < HEADER_SIZE {
            return Err(Error::ShortDockImageHeader(header.len()));
        }
        let total = u32::from_le_bytes(header[0..4].try_into().expect("four-byte slice"));
        let width = u16::from_le_bytes(header[4..6].try_into().expect("two-byte slice"));
        let height = u16::from_le_bytes(header[6..8].try_into().expect("two-byte slice"));
        let format = header[8];
        if (total as usize, width, height, format)
            != (STORED_IMAGE_BYTES, IMAGE_WIDTH, IMAGE_HEIGHT, IMAGE_FORMAT)
        {
            return Err(Error::InvalidDockImageHeader {
                total,
                width,
                height,
                format,
            });
        }

        let mut pixels = Vec::with_capacity(PIXEL_BYTES);
        let mut offset = HEADER_SIZE;
        while pixels.len() < PIXEL_BYTES {
            let length = READ_CHUNK.min(PIXEL_BYTES - pixels.len());
            let chunk = self.read_dock_image_chunk(offset, length)?;
            if chunk.is_empty() {
                return Err(Error::EmptyDockImageChunk(offset as u32));
            }
            let used = length.min(chunk.len());
            pixels.extend_from_slice(&chunk[..used]);
            offset += used;
        }
        assert_eq!(pixels.len(), PIXEL_BYTES);
        Ok(pixels)
    }

    pub fn write_dock_image(&mut self, pixels: &[u8]) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if pixels.len() != PIXEL_BYTES {
            return Err(Error::InvalidDockPixelDataSize {
                actual: pixels.len(),
                expected: PIXEL_BYTES,
            });
        }
        if self.read_dock_image()? == pixels {
            return Ok(WriteOutcome::Unchanged);
        }
        let mut stored = Vec::with_capacity(STORED_IMAGE_BYTES);
        stored.extend_from_slice(&(STORED_IMAGE_BYTES as u32).to_le_bytes());
        stored.extend_from_slice(&IMAGE_WIDTH.to_le_bytes());
        stored.extend_from_slice(&IMAGE_HEIGHT.to_le_bytes());
        stored.push(IMAGE_FORMAT);
        stored.extend_from_slice(pixels);
        assert_eq!(stored.len(), STORED_IMAGE_BYTES);
        for (index, chunk) in stored.chunks(WRITE_CHUNK).enumerate() {
            let offset = index * WRITE_CHUNK;
            let mut payload = Vec::with_capacity(5 + chunk.len());
            payload.push(0);
            payload.extend_from_slice(&(offset as u32).to_le_bytes());
            payload.extend_from_slice(chunk);
            self.request(WRITE_IMAGE, &payload)?;
        }
        if self.read_dock_image()? != pixels {
            return Err(Error::VerificationFailed {
                target: "dock image",
            });
        }
        Ok(WriteOutcome::Written)
    }

    fn read_dock_image_chunk(&mut self, offset: usize, length: usize) -> Result<Vec<u8>> {
        assert!(offset <= STORED_IMAGE_BYTES);
        assert!(length <= READ_CHUNK);
        let mut payload = Vec::with_capacity(9);
        payload.push(0);
        payload.extend_from_slice(&(offset as u32).to_le_bytes());
        payload.extend_from_slice(&(length as u32).to_le_bytes());
        self.request(READ_IMAGE, &payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_fields_have_confirmed_order() {
        let settings = DockSettings::parse(&[0xdc, 0x4d, 0, 1, 2, 30, 0, 0, 0]).unwrap();
        assert_eq!(settings.menu_colour, [0xdc, 0x4d, 0]);
        assert_eq!(settings.clock_format, ClockFormat::TwentyFourHour);
        assert_eq!(settings.idle_display, IdleDisplay::Image);
        assert_eq!(settings.idle_seconds, 30);
        assert_eq!(settings.off_seconds, 0);
        assert_eq!(settings.encode(), [0xdc, 0x4d, 0, 1, 2, 30, 0, 0, 0]);
    }
}
