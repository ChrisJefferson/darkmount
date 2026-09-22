// SPDX-License-Identifier: MPL-2.0

use hidapi::{HidApi, HidDevice};
use serde::Serialize;

use crate::device::{PRODUCT_ID, VENDOR_ID};
use crate::{Error, Result};

const LAMP_INTERFACE: i32 = 3;
const LAMP_USAGE_PAGE: u16 = 0x59;
const LAMP_USAGE: u16 = 1;
const REPORT_ATTRIBUTES: u8 = 1;
const REPORT_LAMP_REQUEST: u8 = 2;
const REPORT_LAMP_RESPONSE: u8 = 3;
const REPORT_MULTI_UPDATE: u8 = 4;
const REPORT_RANGE_UPDATE: u8 = 5;
const REPORT_CONTROL: u8 = 6;
const ATTRIBUTES_SIZE: usize = 22;
const LAMP_REQUEST_SIZE: usize = 2;
const LAMP_RESPONSE_SIZE: usize = 27;
const MULTI_UPDATE_SIZE: usize = 42;
const RANGE_UPDATE_SIZE: usize = 8;
const CONTROL_SIZE: usize = 1;
const UPDATE_COMPLETE: u8 = 1;
const MULTI_UPDATE_MAX: usize = 8;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct LampArrayInfo {
    pub lamp_count: u16,
    pub width_micrometres: u32,
    pub height_micrometres: u32,
    pub depth_micrometres: u32,
    pub kind: u32,
    pub minimum_update_interval_microseconds: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct LampInfo {
    pub id: u16,
    pub x_micrometres: u32,
    pub y_micrometres: u32,
    pub z_micrometres: u32,
    pub update_latency_microseconds: u32,
    pub purposes: u32,
}

pub struct LampArray {
    device: HidDevice,
    info: LampArrayInfo,
}

#[must_use = "LampArray host control must be explicitly released"]
pub struct LampArrayControl<'a> {
    array: &'a LampArray,
}

impl LampArray {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let mut matches = api.device_list().filter(|candidate| {
            candidate.vendor_id() == VENDOR_ID
                && candidate.product_id() == PRODUCT_ID
                && candidate.interface_number() == LAMP_INTERFACE
                && candidate.usage_page() == LAMP_USAGE_PAGE
                && candidate.usage() == LAMP_USAGE
        });
        let candidate = matches.next().ok_or(Error::LampArrayInterfaceNotFound)?;
        if matches.next().is_some() {
            return Err(Error::MultipleLampArrayInterfaces);
        }
        let device = candidate.open_device(&api)?;
        let attributes = get_feature(&device, REPORT_ATTRIBUTES, ATTRIBUTES_SIZE)?;
        let info = LampArrayInfo {
            lamp_count: u16::from_le_bytes(attributes[0..2].try_into().expect("two-byte slice")),
            width_micrometres: u32::from_le_bytes(
                attributes[2..6].try_into().expect("four-byte slice"),
            ),
            height_micrometres: u32::from_le_bytes(
                attributes[6..10].try_into().expect("four-byte slice"),
            ),
            depth_micrometres: u32::from_le_bytes(
                attributes[10..14].try_into().expect("four-byte slice"),
            ),
            kind: u32::from_le_bytes(attributes[14..18].try_into().expect("four-byte slice")),
            minimum_update_interval_microseconds: u32::from_le_bytes(
                attributes[18..22].try_into().expect("four-byte slice"),
            ),
        };
        Ok(Self { device, info })
    }

    pub fn info(&self) -> &LampArrayInfo {
        &self.info
    }

    pub fn lamps(&self) -> Result<Vec<LampInfo>> {
        let mut lamps = Vec::with_capacity(usize::from(self.info.lamp_count));
        for id in 0..self.info.lamp_count {
            set_feature(
                &self.device,
                REPORT_LAMP_REQUEST,
                &id.to_le_bytes(),
                LAMP_REQUEST_SIZE,
            )?;
            let data = get_feature(&self.device, REPORT_LAMP_RESPONSE, LAMP_RESPONSE_SIZE)?;
            let actual = u16::from_le_bytes(data[0..2].try_into().expect("two-byte slice"));
            if actual != id {
                return Err(Error::WrongLampId {
                    expected: id,
                    actual,
                });
            }
            lamps.push(LampInfo {
                id,
                x_micrometres: u32::from_le_bytes(data[2..6].try_into().expect("four-byte slice")),
                y_micrometres: u32::from_le_bytes(data[6..10].try_into().expect("four-byte slice")),
                z_micrometres: u32::from_le_bytes(
                    data[10..14].try_into().expect("four-byte slice"),
                ),
                update_latency_microseconds: u32::from_le_bytes(
                    data[14..18].try_into().expect("four-byte slice"),
                ),
                purposes: u32::from_le_bytes(data[18..22].try_into().expect("four-byte slice")),
            });
        }
        Ok(lamps)
    }

    pub fn take_control(&self) -> Result<LampArrayControl<'_>> {
        set_feature(&self.device, REPORT_CONTROL, &[0], CONTROL_SIZE)?;
        Ok(LampArrayControl { array: self })
    }

    pub fn release(&self) -> Result<()> {
        set_feature(&self.device, REPORT_CONTROL, &[1], CONTROL_SIZE)
    }
}

impl LampArrayControl<'_> {
    pub fn set_solid(&self, colour: [u8; 3]) -> Result<()> {
        self.set_range(0, self.array.info.lamp_count - 1, colour)
    }

    pub fn set_range(&self, first: u16, last: u16, colour: [u8; 3]) -> Result<()> {
        if first > last || last >= self.array.info.lamp_count {
            return Err(Error::InvalidLampRange {
                first,
                last,
                lamp_count: self.array.info.lamp_count,
            });
        }
        let mut payload = Vec::with_capacity(RANGE_UPDATE_SIZE);
        payload.push(UPDATE_COMPLETE);
        payload.extend_from_slice(&first.to_le_bytes());
        payload.extend_from_slice(&last.to_le_bytes());
        payload.extend_from_slice(&colour);
        set_feature(
            &self.array.device,
            REPORT_RANGE_UPDATE,
            &payload,
            RANGE_UPDATE_SIZE,
        )
    }

    pub fn set_lamps(&self, items: &[(u16, [u8; 3])]) -> Result<()> {
        if items.is_empty() || !items.iter().all(|(id, _)| *id < self.array.info.lamp_count) {
            return Err(Error::InvalidLampUpdate);
        }
        for (block_index, block) in items.chunks(MULTI_UPDATE_MAX).enumerate() {
            let final_block = (block_index + 1) * MULTI_UPDATE_MAX >= items.len();
            let mut payload = vec![
                block.len() as u8,
                if final_block { UPDATE_COMPLETE } else { 0 },
            ];
            for (id, _) in block {
                payload.extend_from_slice(&id.to_le_bytes());
            }
            payload.resize(2 + MULTI_UPDATE_MAX * 2, 0);
            for (_, colour) in block {
                payload.extend_from_slice(colour);
            }
            payload.resize(MULTI_UPDATE_SIZE, 0);
            set_feature(
                &self.array.device,
                REPORT_MULTI_UPDATE,
                &payload,
                MULTI_UPDATE_SIZE,
            )?;
        }
        Ok(())
    }

    pub fn release(self) -> Result<()> {
        self.array.release()
    }
}

fn get_feature(device: &HidDevice, report_id: u8, payload_size: usize) -> Result<Vec<u8>> {
    let mut report = vec![0_u8; payload_size + 1];
    report[0] = report_id;
    let actual = device.get_feature_report(&mut report)?;
    if actual != report.len() {
        return Err(Error::InvalidFeatureReportSize {
            report_id,
            actual,
            expected: report.len(),
        });
    }
    Ok(report[1..].to_vec())
}

fn set_feature(
    device: &HidDevice,
    report_id: u8,
    payload: &[u8],
    payload_size: usize,
) -> Result<()> {
    assert!(payload.len() <= payload_size);
    let mut report = vec![0_u8; payload_size + 1];
    report[0] = report_id;
    report[1..1 + payload.len()].copy_from_slice(payload);
    device.send_feature_report(&report)?;
    Ok(())
}
