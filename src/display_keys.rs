// SPDX-License-Identifier: MPL-2.0

use crate::device::Keyboard;
use crate::qlink::Command;
use crate::{Error, Result, WriteOutcome};

const READ_IMAGE: Command = Command::new(0x20, 0x03);
const WRITE_IMAGE: Command = Command::new(0x20, 0x02);
const FIRST_KEY_ID: u8 = 0x6d;
const HEADER_SIZE: usize = 9;
const READ_CHUNK: usize = 54;
const WRITE_CHUNK: usize = 49;
const IMAGE_WIDTH: u16 = 120;
const IMAGE_HEIGHT: u16 = 120;
const IMAGE_FORMAT: u8 = 3;
const MAX_IMAGE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct DisplayKey(u8);

impl DisplayKey {
    pub fn new(number: u8) -> Result<Self> {
        if !(1..=8).contains(&number) {
            return Err(Error::InvalidDisplayKey(number));
        }
        Ok(Self(number))
    }

    pub fn number(self) -> u8 {
        self.0
    }

    fn device_id(self) -> u8 {
        FIRST_KEY_ID + self.0 - 1
    }

    pub(crate) fn from_device_id(id: u8) -> Option<Self> {
        if (FIRST_KEY_ID..FIRST_KEY_ID + 8).contains(&id) {
            Some(Self(id - FIRST_KEY_ID + 1))
        } else {
            None
        }
    }
}

impl Keyboard {
    pub fn read_display_key(&mut self, key: DisplayKey) -> Result<Vec<u8>> {
        self.require_control_support()?;
        let header = self.read_display_key_chunk(key, 0, HEADER_SIZE)?;
        if header.len() < HEADER_SIZE {
            return Err(Error::ShortImageHeader(header.len()));
        }
        let total = u32::from_le_bytes(header[0..4].try_into().expect("four-byte slice"));
        let width = u16::from_le_bytes(header[4..6].try_into().expect("two-byte slice"));
        let height = u16::from_le_bytes(header[6..8].try_into().expect("two-byte slice"));
        let format = header[8];
        if total as usize <= HEADER_SIZE || total as usize > MAX_IMAGE_BYTES {
            return Err(Error::InvalidImageLength(total));
        }
        if (width, height, format) != (IMAGE_WIDTH, IMAGE_HEIGHT, IMAGE_FORMAT) {
            return Err(Error::InvalidDisplayImageHeader {
                width,
                height,
                format,
            });
        }

        let jpeg_size = total as usize - HEADER_SIZE;
        let mut jpeg = Vec::with_capacity(jpeg_size);
        let mut offset = HEADER_SIZE;
        while jpeg.len() < jpeg_size {
            let length = READ_CHUNK.min(jpeg_size - jpeg.len());
            let chunk = self.read_display_key_chunk(key, offset, length)?;
            if chunk.is_empty() {
                return Err(Error::EmptyImageChunk(offset as u32));
            }
            let used = length.min(chunk.len());
            jpeg.extend_from_slice(&chunk[..used]);
            offset += used;
        }
        if !is_complete_jpeg(&jpeg) {
            return Err(Error::InvalidJpeg);
        }
        Ok(jpeg)
    }

    pub fn write_display_key(&mut self, key: DisplayKey, jpeg: &[u8]) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if !is_complete_jpeg(jpeg) {
            return Err(Error::InvalidJpeg);
        }
        if jpeg.len() + HEADER_SIZE > MAX_IMAGE_BYTES {
            return Err(Error::DisplayImageTooLarge {
                actual: jpeg.len(),
                maximum: MAX_IMAGE_BYTES - HEADER_SIZE,
            });
        }
        if self.read_display_key(key)? == jpeg {
            return Ok(WriteOutcome::Unchanged);
        }

        let mut stored = Vec::with_capacity(HEADER_SIZE + jpeg.len());
        stored.extend_from_slice(&((HEADER_SIZE + jpeg.len()) as u32).to_le_bytes());
        stored.extend_from_slice(&IMAGE_WIDTH.to_le_bytes());
        stored.extend_from_slice(&IMAGE_HEIGHT.to_le_bytes());
        stored.push(IMAGE_FORMAT);
        stored.extend_from_slice(jpeg);
        for (index, chunk) in stored.chunks(WRITE_CHUNK).enumerate() {
            let offset = index * WRITE_CHUNK;
            let mut payload = Vec::with_capacity(6 + chunk.len());
            payload.extend_from_slice(&[key.device_id(), 0]);
            payload.extend_from_slice(&(offset as u32).to_le_bytes());
            payload.extend_from_slice(chunk);
            self.request(WRITE_IMAGE, &payload)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        if self.read_display_key(key)? != jpeg {
            return Err(Error::VerificationFailed {
                target: "display-key image",
            });
        }
        Ok(WriteOutcome::Written)
    }

    fn read_display_key_chunk(
        &mut self,
        key: DisplayKey,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>> {
        assert!(offset <= MAX_IMAGE_BYTES);
        assert!(length <= READ_CHUNK);
        let mut payload = Vec::with_capacity(7);
        payload.extend_from_slice(&[key.device_id(), 0]);
        payload.extend_from_slice(&(offset as u32).to_le_bytes());
        payload.push(length as u8);
        self.request(READ_IMAGE, &payload)
    }
}

fn is_complete_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_key_ids_are_bounded() {
        assert_eq!(DisplayKey::new(1).unwrap().device_id(), 0x6d);
        assert_eq!(DisplayKey::new(8).unwrap().device_id(), 0x74);
        assert!(DisplayKey::new(0).is_err());
        assert!(DisplayKey::new(9).is_err());
    }

    #[test]
    fn jpeg_requires_both_markers() {
        assert!(is_complete_jpeg(&[0xff, 0xd8, 1, 0xff, 0xd9]));
        assert!(!is_complete_jpeg(&[0xff, 0xd8, 1]));
    }
}
