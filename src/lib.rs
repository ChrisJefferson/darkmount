// SPDX-License-Identifier: MPL-2.0

pub mod device;
pub mod display_keys;
pub mod dock;
pub mod error;
pub mod events;
pub mod images;
pub mod lamp_array;
pub mod lighting;
mod qlink;
pub mod support;
mod transport;

pub use error::{Error, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WriteOutcome {
    Unchanged,
    Written,
}
