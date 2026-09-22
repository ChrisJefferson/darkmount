// SPDX-License-Identifier: MPL-2.0

use hidapi::{HidApi, HidError};
use serde::Serialize;

pub const VENDOR_ID: u16 = 0x373f;
pub const PRODUCT_ID: u16 = 0x0001;
pub const CONTROL_INTERFACE: i32 = 2;
pub const CONTROL_USAGE_PAGE: u16 = 0xff00;
pub const CONTROL_USAGE: u16 = 1;

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

pub fn enumerate() -> Result<Vec<HidCollection>, HidError> {
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
}
