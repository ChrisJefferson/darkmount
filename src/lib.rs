// SPDX-License-Identifier: MPL-2.0

pub mod assignments;
pub mod device;
pub mod display_keys;
pub mod dock;
pub mod error;
pub mod events;
pub mod game_mode;
pub mod images;
pub mod input_trace;
pub mod lamp_array;
pub mod lighting;
mod qlink;
pub mod support;
mod transport;

pub use error::{Error, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOutcome {
    Unchanged,
    Written,
}
