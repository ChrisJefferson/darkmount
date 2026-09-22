// SPDX-License-Identifier: MPL-2.0

use std::fs::{File, OpenOptions};
use std::time::Duration;

use fs2::FileExt;
use hidapi::{HidApi, HidDevice};

use crate::device::{CONTROL_INTERFACE, CONTROL_USAGE, CONTROL_USAGE_PAGE, PRODUCT_ID, VENDOR_ID};
use crate::{Error, Result};

pub(crate) const REPORT_SIZE: usize = 64;

pub(crate) trait Transport {
    fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> Result<()>;
    fn read_report(&mut self, timeout: Duration) -> Result<Option<[u8; REPORT_SIZE]>>;
}

pub(crate) struct HidTransport {
    device: HidDevice,
    _lock: DeviceLock,
}

struct DeviceLock {
    _file: File,
}

impl DeviceLock {
    fn acquire() -> Result<Self> {
        let path = std::env::temp_dir().join("darkmount-373f-0001.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Err(Error::DeviceBusy),
            Err(error) => Err(Error::Io(error)),
        }
    }
}

impl HidTransport {
    pub(crate) fn open() -> Result<Self> {
        let lock = DeviceLock::acquire()?;
        let api = HidApi::new()?;
        let mut matches = api.device_list().filter(|candidate| {
            candidate.vendor_id() == VENDOR_ID
                && candidate.product_id() == PRODUCT_ID
                && candidate.interface_number() == CONTROL_INTERFACE
                && candidate.usage_page() == CONTROL_USAGE_PAGE
                && candidate.usage() == CONTROL_USAGE
        });
        let info = matches.next().ok_or(Error::ControlInterfaceNotFound)?;
        if matches.next().is_some() {
            return Err(Error::MultipleControlInterfaces);
        }
        let device = info.open_device(&api)?;
        device.set_blocking_mode(false)?;
        Ok(Self {
            device,
            _lock: lock,
        })
    }
}

impl Transport for HidTransport {
    fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> Result<()> {
        let mut with_report_id = [0_u8; REPORT_SIZE + 1];
        with_report_id[1..].copy_from_slice(report);
        let actual = self.device.write(&with_report_id)?;
        if actual != with_report_id.len() {
            return Err(Error::IncompleteWrite {
                actual,
                expected: with_report_id.len(),
            });
        }
        Ok(())
    }

    fn read_report(&mut self, timeout: Duration) -> Result<Option<[u8; REPORT_SIZE]>> {
        let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let mut report = [0_u8; REPORT_SIZE];
        let actual = self.device.read_timeout(&mut report, timeout_ms)?;
        if actual == 0 {
            return Ok(None);
        }
        if actual != REPORT_SIZE {
            return Err(Error::InvalidReportSize(actual));
        }
        Ok(Some(report))
    }
}
