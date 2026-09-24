// SPDX-License-Identifier: MPL-2.0

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Hid(#[from] hidapi::HidError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error("no Dark Mount vendor control interface was found")]
    ControlInterfaceNotFound,
    #[error("multiple Dark Mount vendor control interfaces were found")]
    MultipleControlInterfaces,
    #[error("another darkmount process is using the vendor control interface")]
    DeviceBusy,
    #[error("no Dark Mount LampArray interface was found")]
    LampArrayInterfaceNotFound,
    #[error("multiple Dark Mount LampArray interfaces were found")]
    MultipleLampArrayInterfaces,
    #[error("no Dark Mount keyboard input interfaces were found")]
    InputInterfacesNotFound,
    #[error("could not open Dark Mount input interface {interface_number} at {path}: {source}")]
    InputInterfaceOpen {
        interface_number: i32,
        path: String,
        source: hidapi::HidError,
    },
    #[error("HID report write was incomplete: wrote {actual} of {expected} bytes")]
    IncompleteWrite { actual: usize, expected: usize },
    #[error("HID report had {0} bytes; expected 64")]
    InvalidReportSize(usize),
    #[error("QLink frame has invalid length marker {0}")]
    InvalidFrameLength(u8),
    #[error("QLink frame CRC mismatch: received {received:#06x}, calculated {calculated:#06x}")]
    CrcMismatch { received: u16, calculated: u16 },
    #[error("QLink payload has {actual} bytes; maximum is {maximum}")]
    PayloadTooLarge { actual: usize, maximum: usize },
    #[error("timed out waiting for QLink command {group:02x}/{command:02x}, sequence {sequence}")]
    Timeout {
        group: u8,
        command: u8,
        sequence: u8,
    },
    #[error("device rejected QLink command {group:02x}/{command:02x} with status {status}")]
    DeviceStatus { group: u8, command: u8, status: u8 },
    #[error("too many unmatched QLink reports are pending")]
    PendingOverflow,
    #[error("timed out waiting for QLink continuation fragment {0}")]
    ContinuationTimeout(u8),
    #[error("QLink continuation has invalid last-byte marker {0}")]
    InvalidContinuationLength(u8),
    #[error("QLink continuation fragment is {actual}; expected {expected}")]
    WrongContinuationIndex { actual: u8, expected: u8 },
    #[error("QLink continuation session is {actual}; expected {expected}")]
    WrongContinuationSession { actual: u8, expected: u8 },
    #[error("session response has {0} payload bytes; expected at least 7")]
    ShortSessionResponse(usize),
    #[error("session response did not echo the nonce")]
    SessionNonceMismatch,
    #[error("device returned invalid session zero")]
    InvalidSession,
    #[error("operating-system randomness failed: {0}")]
    Random(String),
    #[error("device information has {0} bytes; expected at least 4")]
    ShortDeviceInfo(usize),
    #[error("device information has {actual} bytes; expected at least {expected}")]
    IncompleteDeviceInfo { actual: usize, expected: usize },
    #[error("serial-number payload is empty")]
    EmptySerial,
    #[error("serial-number payload has {actual} bytes; expected {expected}")]
    IncompleteSerial { actual: usize, expected: usize },
    #[error("display key must be between 1 and 8; received {0}")]
    InvalidDisplayKey(u8),
    #[error("assignment response has {0} bytes; expected at least 2")]
    ShortAssignmentResponse(usize),
    #[error("assignment response ended part-way through an entry after {0} bytes")]
    IncompleteAssignmentResponse(usize),
    #[error("assignment {index} has unknown action type {action_type:#04x}")]
    UnknownAssignmentActionType { index: usize, action_type: u8 },
    #[error("assignment {index} action type {action_type:#04x} contains invalid UTF-8")]
    InvalidAssignmentText { index: usize, action_type: u8 },
    #[error("assignment text has {0} bytes; expected between 1 and 51")]
    InvalidAssignmentTextLength(usize),
    #[error("writing this assignment action has not been verified")]
    UnsupportedAssignmentWrite,
    #[error("display-key standard assignment has unsupported trailing parameter {0:#04x}")]
    UnsupportedStandardKeyParameter(u8),
    #[error("invalid assignment target {0:?}; use display-1..display-8 or a decimal/hex key ID")]
    InvalidAssignmentTarget(String),
    #[error("assignment action is only verified for display keys, not key ID {0:#06x}")]
    AssignmentRequiresDisplayKey(u16),
    #[error("invalid USB HID keycode {0:?}; use a decimal byte or 0x00..0xff")]
    InvalidKeyCode(String),
    #[error("the captured default assignment for key ID {0:#06x} is not known")]
    UnknownDefaultAssignment(u16),
    #[error("failed while assigning display key {display_key} to F{function_key}: {source}")]
    DisplayFunctionKeyPreset {
        display_key: u8,
        function_key: u8,
        source: Box<Error>,
    },
    #[error("assignment response has {actual} bytes; expected {expected}")]
    InvalidAssignmentResponseLength { actual: usize, expected: usize },
    #[error("display-key image header has {0} bytes; expected 9")]
    ShortImageHeader(usize),
    #[error("display-key image has invalid stored length {0}")]
    InvalidImageLength(u32),
    #[error(
        "display-key image header is {width}x{height}, format {format}; expected 120x120, format 3"
    )]
    InvalidDisplayImageHeader { width: u16, height: u16, format: u8 },
    #[error("display-key image transfer returned an empty chunk at offset {0}")]
    EmptyImageChunk(u32),
    #[error("display-key image is not a complete JPEG")]
    InvalidJpeg,
    #[error("display-key image has {actual} bytes; maximum JPEG size is {maximum}")]
    DisplayImageTooLarge { actual: usize, maximum: usize },
    #[error("{target} readback differed after writing")]
    VerificationFailed { target: &'static str },
    #[error(
        "refusing control for model {model}, hardware revision {revision}, firmware {versions:?}"
    )]
    UnsupportedDevice {
        model: u16,
        revision: u8,
        versions: Vec<String>,
    },
    #[error("output file already exists: {0}")]
    OutputExists(PathBuf),
    #[error("dock settings have {0} bytes; expected at least 9")]
    ShortDockSettings(usize),
    #[error("dock returned unsupported clock format {0}")]
    InvalidClockFormat(u8),
    #[error("dock returned unsupported idle display {0}")]
    InvalidIdleDisplay(u8),
    #[error("dock image header has {0} bytes; expected 9")]
    ShortDockImageHeader(usize),
    #[error(
        "dock image header is {total} bytes, {width}x{height}, format {format}; expected 153609 bytes, 320x240, format 1"
    )]
    InvalidDockImageHeader {
        total: u32,
        width: u16,
        height: u16,
        format: u8,
    },
    #[error("dock image transfer returned an empty chunk at offset {0}")]
    EmptyDockImageChunk(u32),
    #[error("dock RGB565 image has {actual} bytes; expected {expected}")]
    InvalidDockPixelDataSize { actual: usize, expected: usize },
    #[error("dock did not confirm the clock value it was sent")]
    ClockConfirmationFailed,
    #[error("HID feature report {report_id} has {actual} bytes; expected {expected}")]
    InvalidFeatureReportSize {
        report_id: u8,
        actual: usize,
        expected: usize,
    },
    #[error("LampArray returned lamp {actual} after requesting lamp {expected}")]
    WrongLampId { expected: u16, actual: u16 },
    #[error("invalid LampArray range {first}..={last} for {lamp_count} lamps")]
    InvalidLampRange {
        first: u16,
        last: u16,
        lamp_count: u16,
    },
    #[error("LampArray update list must be non-empty and contain only known lamp IDs")]
    InvalidLampUpdate,
    #[error("colour must contain exactly three bytes")]
    InvalidColour,
    #[error("brightness and speed must be between 0 and 100")]
    InvalidEffectLevel,
    #[error("onboard-effect brightness must be between 10 and 100")]
    InvalidEffectBrightness,
    #[error("onboard-effect speed must be 0 for Static, or between 10 and 100 otherwise")]
    InvalidEffectSpeed,
    #[error("that colour mode has not been verified for the selected onboard effect")]
    UnsupportedEffectColours,
    #[error("the selected direction is not valid for this onboard effect")]
    InvalidEffectDirection,
    #[error("a gradient requires 3 to 8 stops with strictly increasing positions from 0 to 100")]
    InvalidEffectGradient,
    #[error("lighting mode response is invalid: {0:02x?}")]
    InvalidLightingModeResponse(Vec<u8>),
    #[error("game-mode response is invalid: {0:02x?}")]
    InvalidGameModeResponse(Vec<u8>),
    #[error("display-key event payload has {0} bytes; expected at least 5")]
    ShortKeyEvent(usize),
    #[error("display-key event contains unknown key ID {0:#04x}")]
    UnknownEventKey(u8),
    #[error("zoom must be finite and at least 1.0")]
    InvalidZoom,
    #[error("decoded image is {width}x{height}; expected {expected_width}x{expected_height}")]
    InvalidDecodedImageSize {
        width: u32,
        height: u32,
        expected_width: u32,
        expected_height: u32,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
