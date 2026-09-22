// SPDX-License-Identifier: MPL-2.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use darkmount::device::{self, Keyboard};
use darkmount::display_keys::DisplayKey;
use darkmount::dock::{ClockFormat, DockSettings, IdleDisplay};
use darkmount::events::EventStream;
use darkmount::images;
use darkmount::lamp_array::LampArray;
use darkmount::lighting::OnboardEffect;
use darkmount::support::{FirmwareVersion, control_support};
use darkmount::{Error, Result, WriteOutcome};
use image::{DynamicImage, ImageFormat};

#[derive(Parser)]
#[command(name = "darkmount")]
#[command(about = "Control a be quiet! Dark Mount keyboard")]
struct Arguments {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enumerate the Dark Mount HID collections without opening them.
    Devices,
    /// Read model, hardware revision, serial number and firmware versions.
    Info,
    /// Read or write a display-key image.
    #[command(subcommand)]
    DisplayKey(DisplayKeyCommand),
    /// Read or change Media Dock state.
    #[command(subcommand)]
    Dock(DockCommand),
    /// Inspect or temporarily control the standard HID LampArray.
    #[command(subcommand)]
    LampArray(LampArrayCommand),
    /// Set one of the keyboard's persistent onboard lighting effects.
    Lighting {
        #[arg(value_enum)]
        effect: EffectArgument,
        #[arg(long = "colour", default_value = "#ff2800")]
        colours: Vec<String>,
        #[arg(long, default_value_t = 100)]
        brightness: u8,
        #[arg(long, default_value_t = 50)]
        speed: u8,
    },
    /// Print display-key events until interrupted.
    Events,
    /// Report whether a firmware version is supported for control operations.
    Support { version: FirmwareVersion },
}

#[derive(Subcommand)]
enum DisplayKeyCommand {
    /// Read one key as an upright image, or its exact stored JPEG with --raw.
    Read {
        key: u8,
        output: PathBuf,
        #[arg(long)]
        raw: bool,
        #[arg(long)]
        force: bool,
    },
    /// Prepare, write and byte-verify an image.
    Write {
        key: u8,
        input: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        zoom: f32,
        /// Treat input as an already rotated, device-ready JPEG.
        #[arg(long)]
        raw: bool,
        /// Save the current stored JPEG here before an actual write.
        #[arg(long)]
        backup: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DockCommand {
    /// Read the dock's current settings.
    Settings,
    /// Change selected dock settings and verify them by reading them back.
    Configure {
        #[arg(long)]
        colour: Option<String>,
        #[arg(long, value_enum)]
        display: Option<DisplayArgument>,
        #[arg(long = "clock-format", value_parser = clap::value_parser!(u8).range(12..=24))]
        clock_format: Option<u8>,
        #[arg(long)]
        idle_seconds: Option<u16>,
        #[arg(long)]
        off_seconds: Option<u16>,
    },
    /// Read the dock image as PNG, or exact RGB565 bytes with --raw.
    ReadImage {
        output: PathBuf,
        #[arg(long)]
        raw: bool,
        #[arg(long)]
        force: bool,
    },
    /// Prepare, write and byte-verify a dock image.
    WriteImage {
        input: PathBuf,
        /// Treat input as exactly 153600 bytes of RGB565 data.
        #[arg(long)]
        raw: bool,
        /// Save the current dock image as PNG before an actual write.
        #[arg(long)]
        backup: Option<PathBuf>,
    },
    /// Set the dock clock from the Mac's current local time and confirm it.
    SetTime,
}

#[derive(Subcommand)]
enum LampArrayCommand {
    /// Read the LampArray dimensions and update interval.
    Info,
    /// Read the position and purpose of all lamps.
    Lamps,
    /// Show one colour temporarily, then return control to the keyboard.
    Solid {
        colour: String,
        #[arg(long, default_value_t = 5.0)]
        seconds: f32,
    },
    /// Return control to the keyboard immediately.
    Release,
}

#[derive(Clone, Copy, ValueEnum)]
enum DisplayArgument {
    Clock,
    Image,
}

#[derive(Clone, Copy, ValueEnum)]
enum EffectArgument {
    Static,
    ColourWave,
    Tornado,
    Breathing,
    Reactive,
    Matrix,
}

impl From<EffectArgument> for OnboardEffect {
    fn from(value: EffectArgument) -> Self {
        match value {
            EffectArgument::Static => Self::Static,
            EffectArgument::ColourWave => Self::ColourWave,
            EffectArgument::Tornado => Self::Tornado,
            EffectArgument::Breathing => Self::Breathing,
            EffectArgument::Reactive => Self::Reactive,
            EffectArgument::Matrix => Self::Matrix,
        }
    }
}

fn main() {
    if let Err(error) = run(Arguments::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Arguments) -> Result<()> {
    match arguments.command {
        Command::Devices => devices(arguments.json)?,
        Command::Info => device_info(arguments.json)?,
        Command::DisplayKey(command) => display_key(command)?,
        Command::Dock(command) => dock(command, arguments.json)?,
        Command::LampArray(command) => lamp_array(command, arguments.json)?,
        Command::Lighting {
            effect,
            colours,
            brightness,
            speed,
        } => {
            let colours = colours
                .iter()
                .map(|colour| parse_colour(colour))
                .collect::<Result<Vec<_>>>()?;
            let mut keyboard = Keyboard::open()?;
            keyboard.set_onboard_effect(effect.into(), &colours, brightness, speed)?;
            keyboard.close()?;
            println!("onboard lighting effect accepted by the keyboard");
        }
        Command::Events => events(arguments.json)?,
        Command::Support { version } => {
            let support = control_support(version);
            if arguments.json {
                println!(
                    "{}",
                    serde_json::json!({"firmware": version, "control_support": support})
                );
            } else {
                println!("firmware {version}: {support:?}");
            }
        }
    }
    Ok(())
}

fn devices(json: bool) -> Result<()> {
    let devices = device::enumerate()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&devices).expect("serializable")
        );
    } else if devices.is_empty() {
        println!("No Dark Mount HID collections found.");
    } else {
        for device in devices {
            let kind = if device.is_control_interface {
                "control"
            } else {
                "other"
            };
            println!(
                "interface {} usage {:#06x}/{:#04x} {kind}",
                device.interface_number, device.usage_page, device.usage
            );
        }
    }
    Ok(())
}

fn device_info(json: bool) -> Result<()> {
    let keyboard = Keyboard::open()?;
    let info = keyboard.info();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(info).expect("serializable")
        );
    } else {
        println!("model: {}", info.model);
        println!("hardware revision: {}", info.hardware_revision);
        println!("serial number: {}", info.serial_number);
        for (index, version) in info.firmware_versions.iter().enumerate() {
            println!("firmware MCU{index}: {version}");
        }
    }
    keyboard.close()
}

fn display_key(command: DisplayKeyCommand) -> Result<()> {
    match command {
        DisplayKeyCommand::Read {
            key,
            output,
            raw,
            force,
        } => {
            let key = DisplayKey::new(key)?;
            let mut keyboard = Keyboard::open()?;
            let jpeg = keyboard.read_display_key(key)?;
            keyboard.close()?;
            if raw {
                write_output(&output, &jpeg, force)?;
            } else {
                write_image(
                    &output,
                    DynamicImage::ImageRgb8(images::decode_display_key(&jpeg)?),
                    force,
                )?;
            }
            println!(
                "read display key {} into {}",
                key.number(),
                output.display()
            );
        }
        DisplayKeyCommand::Write {
            key,
            input,
            zoom,
            raw,
            backup,
        } => {
            let key = DisplayKey::new(key)?;
            let jpeg = if raw {
                fs::read(&input)?
            } else {
                images::encode_display_key(&images::load(&input)?, zoom)?
            };
            let mut keyboard = Keyboard::open()?;
            if let Some(path) = backup.as_ref() {
                let old = keyboard.read_display_key(key)?;
                if old != jpeg {
                    write_output(path, &old, false)?;
                }
            }
            let outcome = keyboard.write_display_key(key, &jpeg)?;
            keyboard.close()?;
            report_write(outcome, "display-key image");
        }
    }
    Ok(())
}

fn dock(command: DockCommand, json: bool) -> Result<()> {
    match command {
        DockCommand::Settings => {
            let mut keyboard = Keyboard::open()?;
            let settings = keyboard.read_dock_settings()?;
            keyboard.close()?;
            print_settings(&settings, json);
        }
        DockCommand::Configure {
            colour,
            display,
            clock_format,
            idle_seconds,
            off_seconds,
        } => {
            let mut keyboard = Keyboard::open()?;
            let mut settings = keyboard.read_dock_settings()?;
            if let Some(colour) = colour {
                settings.menu_colour = parse_colour(&colour)?;
            }
            if let Some(display) = display {
                settings.idle_display = match display {
                    DisplayArgument::Clock => IdleDisplay::Clock,
                    DisplayArgument::Image => IdleDisplay::Image,
                };
            }
            if let Some(format) = clock_format {
                settings.clock_format = match format {
                    12 => ClockFormat::TwelveHour,
                    24 => ClockFormat::TwentyFourHour,
                    _ => return Err(Error::InvalidClockFormat(format)),
                };
            }
            if let Some(seconds) = idle_seconds {
                settings.idle_seconds = seconds;
            }
            if let Some(seconds) = off_seconds {
                settings.off_seconds = seconds;
            }
            let outcome = keyboard.write_dock_settings(&settings)?;
            keyboard.close()?;
            report_write(outcome, "dock settings");
        }
        DockCommand::ReadImage { output, raw, force } => {
            let mut keyboard = Keyboard::open()?;
            let pixels = keyboard.read_dock_image()?;
            keyboard.close()?;
            if raw {
                write_output(&output, &pixels, force)?;
            } else {
                write_image(
                    &output,
                    DynamicImage::ImageRgb8(images::decode_dock(&pixels)?),
                    force,
                )?;
            }
            println!("read dock image into {}", output.display());
        }
        DockCommand::WriteImage { input, raw, backup } => {
            let pixels = if raw {
                fs::read(&input)?
            } else {
                images::encode_dock(&images::load(&input)?)
            };
            let mut keyboard = Keyboard::open()?;
            if let Some(path) = backup.as_ref() {
                let old = keyboard.read_dock_image()?;
                if old != pixels {
                    write_image(
                        path,
                        DynamicImage::ImageRgb8(images::decode_dock(&old)?),
                        false,
                    )?;
                }
            }
            let outcome = keyboard.write_dock_image(&pixels)?;
            keyboard.close()?;
            report_write(outcome, "dock image");
        }
        DockCommand::SetTime => {
            let local = chrono::Local::now().naive_local().and_utc().timestamp();
            let timestamp = u32::try_from(local).expect("current local timestamp fits in u32");
            let mut keyboard = Keyboard::open()?;
            if !keyboard.set_dock_local_timestamp(timestamp)? {
                return Err(Error::ClockConfirmationFailed);
            }
            keyboard.close()?;
            println!("dock clock set and confirmed");
        }
    }
    Ok(())
}

fn lamp_array(command: LampArrayCommand, json: bool) -> Result<()> {
    let array = LampArray::open()?;
    match command {
        LampArrayCommand::Info => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(array.info()).expect("serializable")
                );
            } else {
                let info = array.info();
                println!("lamps: {}", info.lamp_count);
                println!(
                    "bounds: {} x {} x {} micrometres",
                    info.width_micrometres, info.height_micrometres, info.depth_micrometres
                );
                println!(
                    "minimum update interval: {} microseconds",
                    info.minimum_update_interval_microseconds
                );
            }
        }
        LampArrayCommand::Lamps => {
            let lamps = array.lamps()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&lamps).expect("serializable")
                );
            } else {
                for lamp in lamps {
                    println!(
                        "{}: ({}, {}, {}) purposes {:#x}",
                        lamp.id,
                        lamp.x_micrometres,
                        lamp.y_micrometres,
                        lamp.z_micrometres,
                        lamp.purposes
                    );
                }
            }
        }
        LampArrayCommand::Solid { colour, seconds } => {
            if !seconds.is_finite() || seconds <= 0.0 {
                return Err(Error::InvalidEffectLevel);
            }
            let control = array.take_control()?;
            if let Err(error) = control.set_solid(parse_colour(&colour)?) {
                control.release()?;
                return Err(error);
            }
            std::thread::sleep(Duration::from_secs_f32(seconds));
            control.release()?;
            println!("LampArray control returned to the keyboard");
        }
        LampArrayCommand::Release => {
            array.release()?;
            println!("LampArray control returned to the keyboard");
        }
    }
    Ok(())
}

fn events(json: bool) -> Result<()> {
    let mut stream = EventStream::open()?;
    loop {
        if let Some(event) = stream.next_event(Duration::from_secs(60))? {
            if json {
                println!("{}", serde_json::to_string(&event).expect("serializable"));
            } else {
                let state = if event.pressed { "pressed" } else { "released" };
                println!("display key {} {state}", event.key);
            }
        }
    }
}

fn print_settings(settings: &DockSettings, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(settings).expect("serializable")
        );
    } else {
        println!(
            "menu colour: #{:02x}{:02x}{:02x}",
            settings.menu_colour[0], settings.menu_colour[1], settings.menu_colour[2]
        );
        println!("clock format: {:?}", settings.clock_format);
        println!("idle display: {:?}", settings.idle_display);
        println!("idle image after: {} seconds", settings.idle_seconds);
        println!("display off after: {} seconds", settings.off_seconds);
    }
}

fn parse_colour(text: &str) -> Result<[u8; 3]> {
    let text = text.strip_prefix('#').unwrap_or(text);
    if text.len() != 6 {
        return Err(Error::InvalidColour);
    }
    let red = u8::from_str_radix(&text[0..2], 16).map_err(|_| Error::InvalidColour)?;
    let green = u8::from_str_radix(&text[2..4], 16).map_err(|_| Error::InvalidColour)?;
    let blue = u8::from_str_radix(&text[4..6], 16).map_err(|_| Error::InvalidColour)?;
    Ok([red, green, blue])
}

fn write_output(path: &Path, bytes: &[u8], force: bool) -> Result<()> {
    let mut file = create_output(path, force)?;
    file.write_all(bytes)?;
    Ok(())
}

fn write_image(path: &Path, image: DynamicImage, force: bool) -> Result<()> {
    let format = ImageFormat::from_path(path)?;
    let mut file = create_output(path, force)?;
    image.write_to(&mut file, format)?;
    Ok(())
}

fn create_output(path: &Path, force: bool) -> Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::OutputExists(path.to_path_buf())
        } else {
            Error::Io(error)
        }
    })
}

fn report_write(outcome: WriteOutcome, target: &str) {
    match outcome {
        WriteOutcome::Unchanged => println!("{target} already identical; nothing written"),
        WriteOutcome::Written => println!("{target} written and verified byte for byte"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_strictly_parsed() {
        assert_eq!(parse_colour("#ff2800").unwrap(), [255, 40, 0]);
        assert!(parse_colour("orange").is_err());
    }
}
