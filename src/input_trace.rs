// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};
use serde::Serialize;

use crate::device::{HidCollection, PRODUCT_ID, VENDOR_ID};
use crate::{Error, Result};

const GENERIC_DESKTOP_PAGE: u16 = 0x0001;
const CONSUMER_PAGE: u16 = 0x000c;
const MAX_REPORT_SIZE: usize = 1024;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct InputUsage {
    pub page: u16,
    pub usage: u16,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct InputReport {
    pub elapsed_microseconds: u128,
    pub path: String,
    pub interface_number: i32,
    pub usages: Vec<InputUsage>,
    pub bytes: Vec<u8>,
}

struct InputDevice {
    path: String,
    interface_number: i32,
    usages: Vec<InputUsage>,
    device: HidDevice,
}

pub struct InputTrace {
    devices: Vec<InputDevice>,
    started: Instant,
}

impl InputTrace {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let collections = api
            .device_list()
            .filter(|device| device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID)
            .map(|device| HidCollection {
                path: device.path().to_string_lossy().into_owned(),
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                interface_number: device.interface_number(),
                usage_page: device.usage_page(),
                usage: device.usage(),
                product: device.product_string().map(str::to_owned),
                serial_number: device.serial_number().map(str::to_owned),
                is_control_interface: false,
            })
            .collect::<Vec<_>>();
        let grouped = group_input_collections(&collections);
        if grouped.is_empty() {
            return Err(Error::InputInterfacesNotFound);
        }
        let mut devices = Vec::with_capacity(grouped.len());
        for (path, (interface_number, usages)) in grouped {
            let path_c = CString::new(path.as_bytes()).expect("HID paths cannot contain NUL bytes");
            let device = api
                .open_path(&path_c)
                .map_err(|source| Error::InputInterfaceOpen {
                    interface_number,
                    path: path.clone(),
                    source,
                })?;
            device.set_blocking_mode(false)?;
            devices.push(InputDevice {
                path,
                interface_number,
                usages,
                device,
            });
        }
        Ok(Self {
            devices,
            started: Instant::now(),
        })
    }

    pub fn collections(&self) -> impl Iterator<Item = (&str, i32, &[InputUsage])> {
        self.devices.iter().map(|device| {
            (
                device.path.as_str(),
                device.interface_number,
                device.usages.as_slice(),
            )
        })
    }

    pub fn poll(&self) -> Result<Vec<InputReport>> {
        let mut reports = Vec::new();
        for input in &self.devices {
            let mut buffer = [0; MAX_REPORT_SIZE];
            let length = input.device.read(&mut buffer)?;
            if length != 0 {
                reports.push(InputReport {
                    elapsed_microseconds: self.started.elapsed().as_micros(),
                    path: input.path.clone(),
                    interface_number: input.interface_number,
                    usages: input.usages.clone(),
                    bytes: buffer[..length].to_vec(),
                });
            }
        }
        Ok(reports)
    }

    pub fn capture<F>(&self, duration: Duration, mut report: F) -> Result<usize>
    where
        F: FnMut(InputReport) -> Result<()>,
    {
        let deadline = Instant::now() + duration;
        let mut count = 0;
        while Instant::now() < deadline {
            for input_report in self.poll()? {
                report(input_report)?;
                count += 1;
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(count)
    }
}

fn group_input_collections(
    collections: &[HidCollection],
) -> BTreeMap<String, (i32, Vec<InputUsage>)> {
    let mut grouped = BTreeMap::new();
    for collection in collections {
        if !matches!(collection.usage_page, GENERIC_DESKTOP_PAGE | CONSUMER_PAGE) {
            continue;
        }
        let (_, usages) = grouped
            .entry(collection.path.clone())
            .or_insert_with(|| (collection.interface_number, Vec::new()));
        let usage = InputUsage {
            page: collection.usage_page,
            usage: collection.usage,
        };
        if !usages.contains(&usage) {
            usages.push(usage);
        }
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_collections_are_grouped_by_physical_path() {
        let collections = vec![
            collection("keyboard", 1, GENERIC_DESKTOP_PAGE, 6),
            collection("keyboard", 1, CONSUMER_PAGE, 1),
            collection("control", 2, 0xff00, 1),
        ];
        let grouped = group_input_collections(&collections);
        assert_eq!(grouped.len(), 1);
        assert_eq!(
            grouped["keyboard"],
            (
                1,
                vec![
                    InputUsage { page: 1, usage: 6 },
                    InputUsage {
                        page: 0x0c,
                        usage: 1
                    }
                ]
            )
        );
    }

    fn collection(path: &str, interface_number: i32, usage_page: u16, usage: u16) -> HidCollection {
        HidCollection {
            path: path.to_owned(),
            vendor_id: VENDOR_ID,
            product_id: PRODUCT_ID,
            interface_number,
            usage_page,
            usage,
            product: None,
            serial_number: None,
            is_control_interface: false,
        }
    }
}
