// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl FirmwareVersion {
    pub const CONTROL_SUPPORTED: Self = Self {
        major: 1,
        minor: 29,
        patch: 0,
    };

    pub fn control_is_supported(self) -> bool {
        self == Self::CONTROL_SUPPORTED
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for FirmwareVersion {
    type Err = &'static str;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut fields = text.split('.');
        let major = fields.next().ok_or("missing major version")?;
        let minor = fields.next().ok_or("missing minor version")?;
        let patch = fields.next().ok_or("missing patch version")?;
        if fields.next().is_some() {
            return Err("too many version components");
        }
        Ok(Self {
            major: major.parse().map_err(|_| "invalid major version")?,
            minor: minor.parse().map_err(|_| "invalid minor version")?,
            patch: patch.parse().map_err(|_| "invalid patch version")?,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSupport {
    Supported,
    IdentificationOnly,
}

pub fn control_support(version: FirmwareVersion) -> ControlSupport {
    if version.control_is_supported() {
        ControlSupport::Supported
    } else {
        ControlSupport::IdentificationOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_firmware_is_control_supported() {
        assert_eq!(
            control_support("1.29.0".parse().unwrap()),
            ControlSupport::Supported
        );
        assert_eq!(
            control_support("1.4.0".parse().unwrap()),
            ControlSupport::IdentificationOnly
        );
        assert_eq!(
            control_support("1.30.0".parse().unwrap()),
            ControlSupport::IdentificationOnly
        );
    }

    #[test]
    fn version_parser_rejects_ambiguous_forms() {
        assert!("1.29".parse::<FirmwareVersion>().is_err());
        assert!("1.29.0.1".parse::<FirmwareVersion>().is_err());
        assert!("1.x.0".parse::<FirmwareVersion>().is_err());
    }
}
