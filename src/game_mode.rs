// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;

use crate::device::Keyboard;
use crate::qlink::Command;
use crate::{Error, Result, WriteOutcome};

const READ_SETTINGS: Command = Command::new(0x07, 0x01);
const WRITE_SETTINGS: Command = Command::new(0x07, 0x03);

const SHIFT_TAB: u8 = 1 << 0;
const ALT_F4: u8 = 1 << 1;
const WINDOWS_KEY: u8 = 1 << 2;
const ALT_TAB: u8 = 1 << 3;
const CAPS_LOCK: u8 = 1 << 4;
const KNOWN_BITS: u8 = SHIFT_TAB | ALT_F4 | WINDOWS_KEY | ALT_TAB | CAPS_LOCK;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub struct GameModeSettings {
    pub disable_shift_tab: bool,
    pub disable_alt_f4: bool,
    pub disable_windows_key: bool,
    pub disable_alt_tab: bool,
    pub disable_caps_lock: bool,
}

impl GameModeSettings {
    fn parse(payload: &[u8]) -> Result<Self> {
        let [1, mask] = payload else {
            return Err(Error::InvalidGameModeResponse(payload.to_vec()));
        };
        if mask & !KNOWN_BITS != 0 {
            return Err(Error::InvalidGameModeResponse(payload.to_vec()));
        }
        Ok(Self {
            disable_shift_tab: mask & SHIFT_TAB != 0,
            disable_alt_f4: mask & ALT_F4 != 0,
            disable_windows_key: mask & WINDOWS_KEY != 0,
            disable_alt_tab: mask & ALT_TAB != 0,
            disable_caps_lock: mask & CAPS_LOCK != 0,
        })
    }

    fn encode(self) -> u8 {
        let mut mask = 0;
        if self.disable_shift_tab {
            mask |= SHIFT_TAB;
        }
        if self.disable_alt_f4 {
            mask |= ALT_F4;
        }
        if self.disable_windows_key {
            mask |= WINDOWS_KEY;
        }
        if self.disable_alt_tab {
            mask |= ALT_TAB;
        }
        if self.disable_caps_lock {
            mask |= CAPS_LOCK;
        }
        mask
    }
}

impl Keyboard {
    pub fn read_game_mode_settings(&mut self) -> Result<GameModeSettings> {
        self.require_control_support()?;
        GameModeSettings::parse(&self.request(READ_SETTINGS, &[])?)
    }

    pub fn write_game_mode_settings(&mut self, settings: GameModeSettings) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if self.read_game_mode_settings()? == settings {
            return Ok(WriteOutcome::Unchanged);
        }
        self.request(WRITE_SETTINGS, &[settings.encode()])?;
        if self.read_game_mode_settings()? != settings {
            return Err(Error::VerificationFailed {
                target: "game-mode settings",
            });
        }
        Ok(WriteOutcome::Written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn individual_capture_bits_decode() {
        let all = GameModeSettings::parse(&[1, 0x1f]).unwrap();
        assert_eq!(all.encode(), 0x1f);

        let shift_tab_allowed = GameModeSettings::parse(&[1, 0x1e]).unwrap();
        assert!(!shift_tab_allowed.disable_shift_tab);
        assert!(shift_tab_allowed.disable_alt_f4);

        let caps_lock_allowed = GameModeSettings::parse(&[1, 0x0f]).unwrap();
        assert!(!caps_lock_allowed.disable_caps_lock);
        assert!(caps_lock_allowed.disable_alt_tab);
    }

    #[test]
    fn unknown_bits_are_not_silently_discarded() {
        assert!(matches!(
            GameModeSettings::parse(&[1, 0x20]),
            Err(Error::InvalidGameModeResponse(_))
        ));
    }
}
