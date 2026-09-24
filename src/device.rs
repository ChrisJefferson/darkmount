// SPDX-License-Identifier: MPL-2.0

use hidapi::{HidApi, HidError};
use serde::Serialize;

use crate::qlink::{Command, QLink};
use crate::support::FirmwareVersion;
use crate::transport::HidTransport;
use crate::{Error, Result};

pub const VENDOR_ID: u16 = 0x373f;
pub const PRODUCT_ID: u16 = 0x0001;
pub const CONTROL_INTERFACE: i32 = 2;
pub const CONTROL_USAGE_PAGE: u16 = 0xff00;
pub const CONTROL_USAGE: u16 = 1;

const READ_INFO: Command = Command::new(0x03, 0x01);
const READ_SERIAL: Command = Command::new(0x03, 0x02);

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct HidCollection {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    pub is_control_interface: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct DeviceInfo {
    pub model: u16,
    pub hardware_revision: u8,
    pub firmware_versions: Vec<FirmwareVersion>,
    pub serial_number: String,
}

impl DeviceInfo {
    pub fn require_control_support(&self) -> Result<()> {
        if self.model == 1
            && self.hardware_revision == 1
            && !self.firmware_versions.is_empty()
            && self
                .firmware_versions
                .iter()
                .all(|version| version.control_is_supported())
        {
            return Ok(());
        }
        Err(Error::UnsupportedDevice {
            model: self.model,
            revision: self.hardware_revision,
            versions: self
                .firmware_versions
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
    }
}

pub fn enumerate() -> std::result::Result<Vec<HidCollection>, HidError> {
    let api = HidApi::new()?;
    let collections = api
        .device_list()
        .filter(|device| device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID)
        .map(|device| {
            let interface_number = device.interface_number();
            let usage_page = device.usage_page();
            let usage = device.usage();
            HidCollection {
                path: device.path().to_string_lossy().into_owned(),
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                interface_number,
                usage_page,
                usage,
                product: device.product_string().map(str::to_owned),
                serial_number: device.serial_number().map(str::to_owned),
                is_control_interface: interface_number == CONTROL_INTERFACE
                    && usage_page == CONTROL_USAGE_PAGE
                    && usage == CONTROL_USAGE,
            }
        })
        .collect();
    Ok(collections)
}

pub struct Keyboard {
    link: QLink<HidTransport>,
    info: DeviceInfo,
}

impl Keyboard {
    pub fn open() -> Result<Self> {
        let mut link = QLink::open()?;
        let info_payload = link.request(READ_INFO, &[])?;
        let serial_payload = link.request(READ_SERIAL, &[])?;
        let info = parse_device_info(&info_payload, &serial_payload)?;
        Ok(Self { link, info })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub(crate) fn require_control_support(&self) -> Result<()> {
        self.info.require_control_support()
    }

    pub(crate) fn request(&mut self, command: Command, payload: &[u8]) -> Result<Vec<u8>> {
        self.link.request(command, payload)
    }

    pub(crate) fn read_continuation(&mut self, expected_index: u8) -> Result<Vec<u8>> {
        self.link.read_continuation(expected_index)
    }

    pub(crate) fn read_notification<F>(
        &mut self,
        command: Command,
        timeout: std::time::Duration,
        predicate: F,
    ) -> Result<Option<crate::qlink::Frame>>
    where
        F: Fn(&crate::qlink::Frame) -> bool,
    {
        self.link.read_notification(command, timeout, predicate)
    }

    pub fn close(mut self) -> Result<()> {
        self.link.close()
    }
}

fn parse_device_info(info: &[u8], serial: &[u8]) -> Result<DeviceInfo> {
    if info.len() < 4 {
        return Err(Error::ShortDeviceInfo(info.len()));
    }
    let model = u16::from_le_bytes([info[0], info[1]]);
    let hardware_revision = info[2];
    let count = usize::from(info[3]);
    let expected = 4 + count * 4;
    if info.len() < expected {
        return Err(Error::IncompleteDeviceInfo {
            actual: info.len(),
            expected,
        });
    }
    let firmware_versions = (0..count)
        .map(|index| {
            let offset = 4 + index * 4;
            FirmwareVersion {
                major: decode_bcd(info[offset + 3]),
                minor: decode_bcd(info[offset + 2]),
                patch: decode_bcd(info[offset + 1]),
            }
        })
        .collect();
    if serial.is_empty() {
        return Err(Error::EmptySerial);
    }
    let serial_length = usize::from(serial[0]);
    if serial.len() < serial_length + 1 {
        return Err(Error::IncompleteSerial {
            actual: serial.len() - 1,
            expected: serial_length,
        });
    }
    let serial_bytes = &serial[1..=serial_length];
    let serial_number = match std::str::from_utf8(serial_bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => serial_bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect(),
    };
    Ok(DeviceInfo {
        model,
        hardware_revision,
        firmware_versions,
        serial_number,
    })
}

fn decode_bcd(value: u8) -> u8 {
    let high = value >> 4;
    let low = value & 0x0f;
    if high <= 9 && low <= 9 {
        high * 10 + low
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_identity_is_narrow() {
        assert_eq!(VENDOR_ID, 0x373f);
        assert_eq!(PRODUCT_ID, 1);
        assert_eq!(CONTROL_INTERFACE, 2);
        assert_eq!(CONTROL_USAGE_PAGE, 0xff00);
        assert_eq!(CONTROL_USAGE, 1);
    }

    #[test]
    fn captured_device_information_is_parsed() {
        let info = hex("01000103000029010000290100002901");
        let serial = hex("0e3030324335333930303032313230");
        let parsed = parse_device_info(&info, &serial).unwrap();
        assert_eq!(parsed.model, 1);
        assert_eq!(parsed.hardware_revision, 1);
        assert_eq!(parsed.firmware_versions, vec!["1.29.0".parse().unwrap(); 3]);
        assert_eq!(parsed.serial_number, "002C5390002120");
        parsed.require_control_support().unwrap();
    }

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
}
