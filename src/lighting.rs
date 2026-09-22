// SPDX-License-Identifier: MPL-2.0

use crate::device::Keyboard;
use crate::qlink::Command;
use crate::{Error, Result};

const SET_EFFECT: Command = Command::new(0x10, 0x06);
const MAX_COLOURS: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum OnboardEffect {
    Static = 0,
    ColourWave = 1,
    Tornado = 2,
    Breathing = 3,
    Reactive = 4,
    Matrix = 5,
}

impl Keyboard {
    pub fn set_onboard_effect(
        &mut self,
        effect: OnboardEffect,
        colours: &[[u8; 3]],
        brightness: u8,
        speed: u8,
    ) -> Result<()> {
        self.require_control_support()?;
        if brightness > 100 || speed > 100 {
            return Err(Error::InvalidEffectLevel);
        }
        if colours.is_empty() || colours.len() > MAX_COLOURS {
            return Err(Error::InvalidEffectColourCount);
        }
        let mut payload = vec![0, effect as u8, 0, brightness, speed];
        match colours.len() {
            1 => {
                payload.push(0);
                payload.extend_from_slice(&colours[0]);
            }
            2 => {
                payload.push(1);
                payload.extend_from_slice(&colours[0]);
                payload.extend_from_slice(&colours[1]);
            }
            count => {
                payload.extend_from_slice(&[2, count as u8]);
                for colour in colours {
                    payload.extend_from_slice(colour);
                }
            }
        }
        self.request(SET_EFFECT, &payload)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_values_match_the_manifest_order() {
        assert_eq!(OnboardEffect::Static as u8, 0);
        assert_eq!(OnboardEffect::Matrix as u8, 5);
    }
}
