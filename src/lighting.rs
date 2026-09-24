// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;

use crate::device::Keyboard;
use crate::qlink::Command;
use crate::{Error, Result, WriteOutcome};

const READ_MASTER: Command = Command::new(0x10, 0x01);
const SET_MASTER: Command = Command::new(0x10, 0x02);
const SET_EFFECT: Command = Command::new(0x10, 0x06);
const MAX_GRADIENT_STOPS: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingMode {
    Off,
    General,
    Custom,
}

impl LightingMode {
    fn parse(payload: &[u8]) -> Result<Self> {
        match payload {
            [0] => Ok(Self::Off),
            [1] => Ok(Self::General),
            [3] => Ok(Self::Custom),
            _ => Err(Error::InvalidLightingModeResponse(payload.to_vec())),
        }
    }

    fn encode(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::General => 1,
            Self::Custom => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum CardinalDirection {
    Up = 0,
    Down = 1,
    Left = 2,
    Right = 3,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum RotationDirection {
    Clockwise = 4,
    CounterClockwise = 5,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OnboardEffect {
    Static,
    ColourWave(CardinalDirection),
    Tornado(RotationDirection),
    Breathing,
    Reactive,
    Matrix,
}

impl OnboardEffect {
    fn values(self) -> (u8, u8) {
        match self {
            Self::Static => (0, 0),
            Self::ColourWave(direction) => (1, direction as u8),
            Self::Tornado(direction) => (2, direction as u8),
            Self::Breathing => (3, 0),
            Self::Reactive => (4, 0),
            Self::Matrix => (5, 1),
        }
    }

    pub fn captured_default_colours(self) -> EffectColours {
        match self {
            Self::Static | Self::ColourWave(_) => EffectColours::Single([0xff, 0x28, 0x00]),
            Self::Tornado(_) | Self::Breathing | Self::Reactive => EffectColours::Gradient(vec![
                GradientStop::new([0xff, 0x00, 0x00], 0),
                GradientStop::new([0xff, 0xff, 0x00], 16),
                GradientStop::new([0x00, 0xff, 0x00], 33),
                GradientStop::new([0x00, 0xff, 0xff], 50),
                GradientStop::new([0x00, 0x00, 0xff], 66),
                GradientStop::new([0xff, 0x00, 0xff], 83),
                GradientStop::new([0xff, 0x00, 0x00], 99),
            ]),
            Self::Matrix => EffectColours::Gradient(vec![
                GradientStop::new([0x0d, 0x02, 0x08], 0),
                GradientStop::new([0x00, 0x3b, 0x00], 33),
                GradientStop::new([0x00, 0x8f, 0x11], 66),
                GradientStop::new([0x00, 0xff, 0x41], 100),
            ]),
        }
    }

    pub fn captured_default_speed(self) -> u8 {
        if matches!(self, Self::Static) { 0 } else { 50 }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct GradientStop {
    pub colour: [u8; 3],
    pub position: u8,
}

impl GradientStop {
    const fn new(colour: [u8; 3], position: u8) -> Self {
        Self { colour, position }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EffectColours {
    Single([u8; 3]),
    Dual([u8; 3], [u8; 3]),
    Gradient(Vec<GradientStop>),
}

impl EffectColours {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Single(colour) => {
                payload.push(0);
                payload.extend_from_slice(colour);
            }
            Self::Dual(first, second) => {
                payload.push(1);
                payload.extend_from_slice(first);
                payload.extend_from_slice(second);
            }
            Self::Gradient(stops) => {
                if !(3..=MAX_GRADIENT_STOPS).contains(&stops.len())
                    || stops.iter().any(|stop| stop.position > 100)
                    || stops
                        .windows(2)
                        .any(|pair| pair[0].position >= pair[1].position)
                {
                    return Err(Error::InvalidEffectGradient);
                }
                payload.extend_from_slice(&[2, stops.len() as u8]);
                for stop in stops {
                    payload.extend_from_slice(&stop.colour);
                    payload.push(stop.position);
                }
            }
        }
        Ok(())
    }
}

impl Keyboard {
    pub fn read_onboard_lighting_mode(&mut self) -> Result<LightingMode> {
        self.require_control_support()?;
        LightingMode::parse(&self.request(READ_MASTER, &[])?)
    }

    pub fn write_onboard_lighting_mode(&mut self, mode: LightingMode) -> Result<WriteOutcome> {
        self.require_control_support()?;
        if self.read_onboard_lighting_mode()? == mode {
            return Ok(WriteOutcome::Unchanged);
        }
        self.request(SET_MASTER, &[mode.encode()])?;
        if self.read_onboard_lighting_mode()? != mode {
            return Err(Error::VerificationFailed {
                target: "onboard lighting mode",
            });
        }
        Ok(WriteOutcome::Written)
    }

    pub fn set_onboard_effect(
        &mut self,
        effect: OnboardEffect,
        colours: &EffectColours,
        brightness: u8,
        speed: u8,
    ) -> Result<()> {
        self.require_control_support()?;
        let payload = effect_payload(effect, colours, brightness, speed)?;
        self.request(SET_EFFECT, &payload)?;
        Ok(())
    }
}

fn effect_payload(
    effect: OnboardEffect,
    colours: &EffectColours,
    brightness: u8,
    speed: u8,
) -> Result<Vec<u8>> {
    if !(10..=100).contains(&brightness) {
        return Err(Error::InvalidEffectBrightness);
    }
    match effect {
        OnboardEffect::Static if speed != 0 => return Err(Error::InvalidEffectSpeed),
        OnboardEffect::Static => {}
        _ if !(10..=100).contains(&speed) => return Err(Error::InvalidEffectSpeed),
        _ => {}
    }
    match effect {
        OnboardEffect::Static if !matches!(colours, EffectColours::Single(_)) => {
            return Err(Error::UnsupportedEffectColours);
        }
        OnboardEffect::ColourWave(_) => {}
        _ if *colours != effect.captured_default_colours() => {
            return Err(Error::UnsupportedEffectColours);
        }
        _ => {}
    }
    let (effect, direction) = effect.values();
    let mut payload = vec![0, effect, direction, brightness, speed];
    colours.encode(&mut payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORANGE: [u8; 3] = [0xff, 0x28, 0x00];

    #[test]
    fn lighting_modes_match_windows_captures() {
        assert_eq!(LightingMode::parse(&[0]).unwrap(), LightingMode::Off);
        assert_eq!(LightingMode::parse(&[1]).unwrap(), LightingMode::General);
        assert_eq!(LightingMode::parse(&[3]).unwrap(), LightingMode::Custom);
        assert!(matches!(
            LightingMode::parse(&[2]),
            Err(Error::InvalidLightingModeResponse(_))
        ));
    }

    #[test]
    fn static_packet_matches_windows_capture() {
        assert_eq!(
            effect_payload(
                OnboardEffect::Static,
                &EffectColours::Single(ORANGE),
                100,
                0
            )
            .unwrap(),
            [0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0xff, 0x28, 0x00]
        );
    }

    #[test]
    fn directions_match_windows_captures() {
        let single = EffectColours::Single(ORANGE);
        let left = effect_payload(
            OnboardEffect::ColourWave(CardinalDirection::Left),
            &single,
            100,
            50,
        )
        .unwrap();
        assert_eq!(&left[..5], &[0, 1, 2, 100, 50]);
        let counter_clockwise = effect_payload(
            OnboardEffect::Tornado(RotationDirection::CounterClockwise),
            &OnboardEffect::Tornado(RotationDirection::CounterClockwise).captured_default_colours(),
            100,
            50,
        )
        .unwrap();
        assert_eq!(&counter_clockwise[..5], &[0, 2, 5, 100, 50]);
        let matrix = effect_payload(
            OnboardEffect::Matrix,
            &OnboardEffect::Matrix.captured_default_colours(),
            100,
            50,
        )
        .unwrap();
        assert_eq!(&matrix[..5], &[0, 5, 1, 100, 50]);
    }

    #[test]
    fn gradient_packet_contains_captured_positions() {
        let colours = EffectColours::Gradient(vec![
            GradientStop {
                colour: [0xff, 0x00, 0x00],
                position: 0,
            },
            GradientStop {
                colour: [0x00, 0xff, 0x00],
                position: 50,
            },
            GradientStop {
                colour: [0x00, 0x00, 0xff],
                position: 100,
            },
        ]);
        assert_eq!(
            effect_payload(
                OnboardEffect::ColourWave(CardinalDirection::Up),
                &colours,
                100,
                50,
            )
            .unwrap(),
            [
                0, 1, 0, 100, 50, 2, 3, 0xff, 0, 0, 0, 0, 0xff, 0, 50, 0, 0, 0xff, 100,
            ]
        );
    }

    #[test]
    fn gradient_positions_must_be_strictly_increasing() {
        let colours = EffectColours::Gradient(vec![
            GradientStop {
                colour: [1, 2, 3],
                position: 0,
            },
            GradientStop {
                colour: [4, 5, 6],
                position: 50,
            },
            GradientStop {
                colour: [7, 8, 9],
                position: 50,
            },
        ]);
        assert!(matches!(
            effect_payload(
                OnboardEffect::ColourWave(CardinalDirection::Up),
                &colours,
                100,
                50,
            ),
            Err(Error::InvalidEffectGradient)
        ));
    }

    #[test]
    fn uncaptured_effect_colour_combinations_are_rejected() {
        assert!(matches!(
            effect_payload(
                OnboardEffect::Matrix,
                &EffectColours::Single(ORANGE),
                100,
                50,
            ),
            Err(Error::UnsupportedEffectColours)
        ));
        assert!(matches!(
            effect_payload(
                OnboardEffect::Static,
                &EffectColours::Single(ORANGE),
                100,
                50
            ),
            Err(Error::InvalidEffectSpeed)
        ));
    }
}
