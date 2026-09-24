// SPDX-License-Identifier: MPL-2.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use darkmount::assignments::{Assignment, AssignmentAction, Modifiers};
use darkmount::device::{self, Keyboard};
use darkmount::display_keys::DisplayKey;
use darkmount::dock::{ClockFormat, DockSettings, IdleDisplay};
use darkmount::events::EventStream;
use darkmount::game_mode::GameModeSettings;
use darkmount::images;
use darkmount::input_trace::{InputReport, InputTrace};
use darkmount::lamp_array::LampArray;
use darkmount::lighting::{
    CardinalDirection, EffectColours, GradientStop, LightingMode, OnboardEffect, RotationDirection,
};
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
    /// Read or change the keyboard's onboard assignments.
    Assignments {
        #[command(subcommand)]
        command: Option<AssignmentCommand>,
    },
    /// Capture all understood read-only state and images into a new directory.
    Snapshot { output: PathBuf },
    /// Read or write a display-key image.
    #[command(subcommand)]
    DisplayKey(DisplayKeyCommand),
    /// Read or change Media Dock state.
    #[command(subcommand)]
    Dock(DockCommand),
    /// Inspect or temporarily control the standard HID LampArray.
    #[command(subcommand)]
    LampArray(LampArrayCommand),
    /// Read or change persistent onboard lighting.
    #[command(subcommand)]
    Lighting(LightingCommand),
    /// Read or change the shortcuts blocked while game mode is active.
    #[command(subcommand)]
    GameMode(GameModeCommand),
    /// Print display-key events until interrupted.
    Events,
    /// Capture raw input reports emitted by the keyboard for a bounded period.
    TraceInput {
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=300))]
        seconds: u64,
    },
    /// Report whether a firmware version is supported for control operations.
    Support { version: FirmwareVersion },
}

#[derive(Subcommand)]
enum LightingCommand {
    /// Read the current onboard lighting mode.
    Status,
    /// Select Off, General or Custom mode and verify it by reading it back.
    #[command(arg_required_else_help = true)]
    Mode {
        #[arg(value_enum)]
        mode: LightingModeArgument,
    },
    /// Set one of the keyboard's persistent onboard effects.
    #[command(arg_required_else_help = true)]
    Effect {
        #[arg(value_enum)]
        effect: EffectArgument,
        #[arg(long = "colour", conflicts_with = "gradient_stops")]
        colours: Vec<String>,
        /// Gradient stop in #RRGGBB@POSITION form, with POSITION from 0 to 100.
        #[arg(long = "gradient-stop", conflicts_with = "colours")]
        gradient_stops: Vec<String>,
        #[arg(long, value_enum)]
        direction: Option<DirectionArgument>,
        #[arg(long, default_value_t = 100)]
        brightness: u8,
        #[arg(long)]
        speed: Option<u8>,
    },
}

#[derive(Subcommand)]
enum AssignmentCommand {
    /// Disable a key.
    Disable {
        /// display-1..display-8, or a decimal/hex key ID such as 0x63.
        target: String,
    },
    /// Assign a standard USB HID keycode and optional modifiers.
    SetKey {
        /// display-1..display-8, or a decimal/hex key ID such as 0x63.
        target: String,
        /// USB HID keycode as a decimal byte or 0x00..0xff.
        key_code: String,
        #[arg(long = "modifier", value_enum)]
        modifiers: Vec<ModifierArgument>,
    },
    /// Assign an application path or file URL.
    Application {
        /// A display key such as display-1.
        target: String,
        value: String,
    },
    /// Assign a website URL.
    Website {
        /// A display key such as display-1.
        target: String,
        value: String,
    },
    /// Assign the observed Media “Next effect” action.
    NextEffect {
        /// A display key such as display-1.
        target: String,
    },
    /// Restore a key whose default action has been captured.
    Default {
        /// Currently 0x63 (F12), whose default was captured explicitly.
        target: String,
    },
    /// Apply a named assignment preset.
    #[command(arg_required_else_help = true)]
    Preset {
        #[arg(value_enum)]
        preset: AssignmentPreset,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum AssignmentPreset {
    /// Assign display keys 1–8 to the standard USB keys F13–F20.
    #[value(name = "f13-f20")]
    F13F20,
}

#[derive(Subcommand)]
enum GameModeCommand {
    /// Read the shortcuts currently configured to be blocked.
    Settings,
    /// Change selected shortcuts and verify the complete settings record.
    Configure {
        #[arg(long, value_enum)]
        shift_tab: Option<GameModeAction>,
        #[arg(long, value_enum)]
        alt_f4: Option<GameModeAction>,
        #[arg(long, value_enum)]
        windows_key: Option<GameModeAction>,
        #[arg(long, value_enum)]
        alt_tab: Option<GameModeAction>,
        #[arg(long, value_enum)]
        caps_lock: Option<GameModeAction>,
    },
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
    Disabled,
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

#[derive(Clone, Copy, ValueEnum)]
enum DirectionArgument {
    Up,
    Down,
    Left,
    Right,
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Copy, ValueEnum)]
enum LightingModeArgument {
    Off,
    General,
    Custom,
}

impl From<LightingModeArgument> for LightingMode {
    fn from(value: LightingModeArgument) -> Self {
        match value {
            LightingModeArgument::Off => Self::Off,
            LightingModeArgument::General => Self::General,
            LightingModeArgument::Custom => Self::Custom,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum GameModeAction {
    Allow,
    Block,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModifierArgument {
    LeftControl,
    LeftShift,
    LeftAlt,
    LeftGui,
    RightControl,
    RightShift,
    RightAlt,
    RightGui,
}

impl From<ModifierArgument> for Modifiers {
    fn from(value: ModifierArgument) -> Self {
        match value {
            ModifierArgument::LeftControl => Self::LEFT_CONTROL,
            ModifierArgument::LeftShift => Self::LEFT_SHIFT,
            ModifierArgument::LeftAlt => Self::LEFT_ALT,
            ModifierArgument::LeftGui => Self::LEFT_GUI,
            ModifierArgument::RightControl => Self::RIGHT_CONTROL,
            ModifierArgument::RightShift => Self::RIGHT_SHIFT,
            ModifierArgument::RightAlt => Self::RIGHT_ALT,
            ModifierArgument::RightGui => Self::RIGHT_GUI,
        }
    }
}

impl GameModeAction {
    fn blocked(self) -> bool {
        matches!(self, Self::Block)
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
        Command::Assignments { command } => assignments(command, arguments.json)?,
        Command::Snapshot { output } => snapshot(&output)?,
        Command::DisplayKey(command) => display_key(command)?,
        Command::Dock(command) => dock(command, arguments.json)?,
        Command::LampArray(command) => lamp_array(command, arguments.json)?,
        Command::Lighting(command) => lighting(command, arguments.json)?,
        Command::GameMode(command) => game_mode(command, arguments.json)?,
        Command::Events => events(arguments.json)?,
        Command::TraceInput { seconds } => {
            trace_input(Duration::from_secs(seconds), arguments.json)?
        }
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

fn assignments(command: Option<AssignmentCommand>, json: bool) -> Result<()> {
    let mut keyboard = Keyboard::open()?;
    match command {
        None => {
            let assignments = keyboard.read_assignments()?;
            keyboard.close()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&assignments).expect("serializable")
                );
            } else {
                for assignment in assignments {
                    print_assignment(assignment);
                }
            }
        }
        Some(command) => {
            let (target, action) = match command {
                AssignmentCommand::Disable { target } => (
                    parse_assignment_target(&target)?,
                    AssignmentAction::Disabled,
                ),
                AssignmentCommand::SetKey {
                    target,
                    key_code,
                    modifiers,
                } => {
                    let mut combined = Modifiers::NONE;
                    for modifier in modifiers {
                        combined |= modifier.into();
                    }
                    (
                        parse_assignment_target(&target)?,
                        AssignmentAction::StandardKey {
                            modifiers: combined,
                            key_code: parse_u8(&key_code)?,
                        },
                    )
                }
                AssignmentCommand::Application { target, value } => (
                    parse_display_assignment_target(&target)?,
                    AssignmentAction::Application { value },
                ),
                AssignmentCommand::Website { target, value } => (
                    parse_display_assignment_target(&target)?,
                    AssignmentAction::Website { value },
                ),
                AssignmentCommand::NextEffect { target } => (
                    parse_display_assignment_target(&target)?,
                    AssignmentAction::NextEffect,
                ),
                AssignmentCommand::Default { target } => {
                    let target = parse_assignment_target(&target)?;
                    let expected = known_default_assignment(target)?;
                    let outcome = keyboard.restore_default_assignment(target, expected.as_ref())?;
                    keyboard.close()?;
                    report_write(outcome, "default key assignment");
                    return Ok(());
                }
                AssignmentCommand::Preset { preset } => {
                    let outcomes = match preset {
                        AssignmentPreset::F13F20 => keyboard.assign_display_function_keys()?,
                    };
                    keyboard.close()?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&outcomes).expect("serializable")
                        );
                    } else {
                        for outcome in outcomes {
                            let status = match outcome.outcome {
                                WriteOutcome::Unchanged => "unchanged",
                                WriteOutcome::Written => "written",
                            };
                            println!(
                                "display key {} -> F{} (HID {:#04x}): {status}",
                                outcome.display_key, outcome.function_key, outcome.hid_key_code
                            );
                        }
                    }
                    return Ok(());
                }
            };
            let outcome = keyboard.write_assignment(target, &action)?;
            keyboard.close()?;
            report_write(outcome, "key assignment");
        }
    }
    Ok(())
}

fn parse_assignment_target(text: &str) -> Result<u16> {
    if let Some(number) = text.strip_prefix("display-") {
        let number = number
            .parse::<u8>()
            .map_err(|_| Error::InvalidAssignmentTarget(text.to_owned()))?;
        let key = DisplayKey::new(number)?;
        return Ok(0x6c + u16::from(key.number()));
    }
    parse_u16(text).map_err(|_| Error::InvalidAssignmentTarget(text.to_owned()))
}

fn parse_display_assignment_target(text: &str) -> Result<u16> {
    let key_id = parse_assignment_target(text)?;
    if !(0x6d..=0x74).contains(&key_id) {
        return Err(Error::AssignmentRequiresDisplayKey(key_id));
    }
    Ok(key_id)
}

fn parse_u8(text: &str) -> Result<u8> {
    let value = parse_u16(text).map_err(|_| Error::InvalidKeyCode(text.to_owned()))?;
    u8::try_from(value).map_err(|_| Error::InvalidKeyCode(text.to_owned()))
}

fn parse_u16(text: &str) -> std::result::Result<u16, std::num::ParseIntError> {
    if let Some(hex) = text.strip_prefix("0x") {
        u16::from_str_radix(hex, 16)
    } else {
        text.parse()
    }
}

fn known_default_assignment(key_id: u16) -> Result<Option<AssignmentAction>> {
    if key_id == 0x63 {
        Ok(None)
    } else {
        Err(Error::UnknownDefaultAssignment(key_id))
    }
}

fn print_assignment(assignment: Assignment) {
    let key = assignment
        .display_key
        .map(|key| format!(" (display key {key})"))
        .unwrap_or_default();
    let action = match assignment.action {
        AssignmentAction::Disabled => "disabled".to_owned(),
        AssignmentAction::StandardKey {
            modifiers,
            key_code,
        } => format!(
            "standard key modifiers={:#04x} key={key_code:#04x}",
            modifiers.bits()
        ),
        AssignmentAction::NextEffect => "media next effect".to_owned(),
        AssignmentAction::Subtype {
            action_type,
            subtype,
        } => format!("action {action_type:#04x}/{subtype:#04x}"),
        AssignmentAction::Application { value } => format!("application {value:?}"),
        AssignmentAction::Website { value } => format!("website {value:?}"),
    };
    println!("{:#06x}: {action}{key}", assignment.key_id);
}

fn snapshot(output: &Path) -> Result<()> {
    let mut keyboard = Keyboard::open()?;
    let device = keyboard.info().clone();
    let assignments = keyboard.read_assignments()?;
    let dock_settings = keyboard.read_dock_settings()?;
    let mut files = Vec::new();
    let mut display_key_files = Vec::new();
    for number in 1..=8 {
        let key = DisplayKey::new(number)?;
        let jpeg = keyboard.read_display_key(key)?;
        let stored = format!("display-keys/key-{number}-stored.jpg");
        let upright = format!("display-keys/key-{number}.png");
        files.push((PathBuf::from(&stored), jpeg.clone()));
        files.push((
            PathBuf::from(&upright),
            images::encode_png(&images::decode_display_key(&jpeg)?)?,
        ));
        display_key_files.push(serde_json::json!({
            "key": number,
            "stored_jpeg": stored,
            "upright_png": upright,
        }));
    }
    let dock_pixels = keyboard.read_dock_image()?;
    keyboard.close()?;
    files.push((PathBuf::from("dock.rgb565"), dock_pixels.clone()));
    files.push((
        PathBuf::from("dock.png"),
        images::encode_png(&images::decode_dock(&dock_pixels)?)?,
    ));
    let manifest = serde_json::json!({
        "format_version": 1,
        "captured_at": chrono::Utc::now().to_rfc3339(),
        "device": device,
        "assignments": assignments,
        "dock_settings": dock_settings,
        "files": {
            "display_keys": display_key_files,
            "dock_rgb565": "dock.rgb565",
            "dock_png": "dock.png",
        },
    });
    files.push((
        PathBuf::from("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("snapshot manifest is serializable"),
    ));

    create_output_directory(output)?;
    fs::create_dir(output.join("display-keys"))?;
    for (relative, bytes) in files {
        write_output(&output.join(relative), &bytes, false)?;
    }
    println!("snapshot captured in {}", output.display());
    Ok(())
}

fn create_output_directory(path: &Path) -> Result<()> {
    fs::create_dir(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::OutputExists(path.to_path_buf())
        } else {
            Error::Io(error)
        }
    })
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
                    DisplayArgument::Disabled => IdleDisplay::Disabled,
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

fn lighting(command: LightingCommand, json: bool) -> Result<()> {
    let mut keyboard = Keyboard::open()?;
    match command {
        LightingCommand::Status => {
            let mode = keyboard.read_onboard_lighting_mode()?;
            keyboard.close()?;
            if json {
                println!("{}", serde_json::json!({"mode": mode}));
            } else {
                println!("onboard lighting mode: {mode:?}");
            }
        }
        LightingCommand::Mode { mode } => {
            let mode = mode.into();
            let outcome = keyboard.write_onboard_lighting_mode(mode)?;
            keyboard.close()?;
            report_write(outcome, "onboard lighting mode");
        }
        LightingCommand::Effect {
            effect,
            colours,
            gradient_stops,
            direction,
            brightness,
            speed,
        } => {
            let effect = parse_effect(effect, direction)?;
            let colours = parse_effect_colours(effect, &colours, &gradient_stops)?;
            let speed = speed.unwrap_or_else(|| effect.captured_default_speed());
            keyboard.set_onboard_effect(effect, &colours, brightness, speed)?;
            keyboard.close()?;
            println!("onboard lighting effect accepted by the keyboard");
        }
    }
    Ok(())
}

fn game_mode(command: GameModeCommand, json: bool) -> Result<()> {
    let mut keyboard = Keyboard::open()?;
    match command {
        GameModeCommand::Settings => {
            let settings = keyboard.read_game_mode_settings()?;
            keyboard.close()?;
            print_game_mode_settings(&settings, json);
        }
        GameModeCommand::Configure {
            shift_tab,
            alt_f4,
            windows_key,
            alt_tab,
            caps_lock,
        } => {
            let mut settings = keyboard.read_game_mode_settings()?;
            if let Some(action) = shift_tab {
                settings.disable_shift_tab = action.blocked();
            }
            if let Some(action) = alt_f4 {
                settings.disable_alt_f4 = action.blocked();
            }
            if let Some(action) = windows_key {
                settings.disable_windows_key = action.blocked();
            }
            if let Some(action) = alt_tab {
                settings.disable_alt_tab = action.blocked();
            }
            if let Some(action) = caps_lock {
                settings.disable_caps_lock = action.blocked();
            }
            let outcome = keyboard.write_game_mode_settings(settings)?;
            keyboard.close()?;
            report_write(outcome, "game-mode settings");
        }
    }
    Ok(())
}

fn print_game_mode_settings(settings: &GameModeSettings, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(settings).expect("serializable")
        );
    } else {
        println!("Shift+Tab blocked: {}", settings.disable_shift_tab);
        println!("Alt+F4 blocked: {}", settings.disable_alt_f4);
        println!("Windows key blocked: {}", settings.disable_windows_key);
        println!("Alt+Tab blocked: {}", settings.disable_alt_tab);
        println!("Caps Lock blocked: {}", settings.disable_caps_lock);
    }
}

fn parse_effect(
    effect: EffectArgument,
    direction: Option<DirectionArgument>,
) -> Result<OnboardEffect> {
    match (effect, direction) {
        (EffectArgument::Static, None) => Ok(OnboardEffect::Static),
        (EffectArgument::ColourWave, None | Some(DirectionArgument::Up)) => {
            Ok(OnboardEffect::ColourWave(CardinalDirection::Up))
        }
        (EffectArgument::ColourWave, Some(DirectionArgument::Down)) => {
            Ok(OnboardEffect::ColourWave(CardinalDirection::Down))
        }
        (EffectArgument::ColourWave, Some(DirectionArgument::Left)) => {
            Ok(OnboardEffect::ColourWave(CardinalDirection::Left))
        }
        (EffectArgument::ColourWave, Some(DirectionArgument::Right)) => {
            Ok(OnboardEffect::ColourWave(CardinalDirection::Right))
        }
        (EffectArgument::Tornado, None | Some(DirectionArgument::Clockwise)) => {
            Ok(OnboardEffect::Tornado(RotationDirection::Clockwise))
        }
        (EffectArgument::Tornado, Some(DirectionArgument::CounterClockwise)) => {
            Ok(OnboardEffect::Tornado(RotationDirection::CounterClockwise))
        }
        (EffectArgument::Breathing, None) => Ok(OnboardEffect::Breathing),
        (EffectArgument::Reactive, None) => Ok(OnboardEffect::Reactive),
        (EffectArgument::Matrix, None) => Ok(OnboardEffect::Matrix),
        _ => Err(Error::InvalidEffectDirection),
    }
}

fn parse_effect_colours(
    effect: OnboardEffect,
    colours: &[String],
    gradient_stops: &[String],
) -> Result<EffectColours> {
    if !gradient_stops.is_empty() {
        let stops = gradient_stops
            .iter()
            .map(|stop| parse_gradient_stop(stop))
            .collect::<Result<Vec<_>>>()?;
        return Ok(EffectColours::Gradient(stops));
    }
    match colours {
        [] => Ok(effect.captured_default_colours()),
        [single] => Ok(EffectColours::Single(parse_colour(single)?)),
        [first, second] => Ok(EffectColours::Dual(
            parse_colour(first)?,
            parse_colour(second)?,
        )),
        _ => Err(Error::InvalidEffectGradient),
    }
}

fn parse_gradient_stop(text: &str) -> Result<GradientStop> {
    let (colour, position) = text.rsplit_once('@').ok_or(Error::InvalidEffectGradient)?;
    let position = position
        .parse::<u8>()
        .map_err(|_| Error::InvalidEffectGradient)?;
    if position > 100 {
        return Err(Error::InvalidEffectGradient);
    }
    Ok(GradientStop {
        colour: parse_colour(colour)?,
        position,
    })
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

fn trace_input(duration: Duration, json: bool) -> Result<()> {
    let trace = InputTrace::open()?;
    if !json {
        for (_, interface_number, usages) in trace.collections() {
            let usages = usages
                .iter()
                .map(|usage| format!("{:#06x}/{:#04x}", usage.page, usage.usage))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("listening on interface {interface_number}, usages {usages}");
        }
    }
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let count = trace.capture(duration, |report| {
        print_input_report(&mut output, &report, json)
    })?;
    eprintln!("captured {count} input reports");
    Ok(())
}

fn print_input_report<W: Write>(output: &mut W, report: &InputReport, json: bool) -> Result<()> {
    if json {
        writeln!(
            output,
            "{}",
            serde_json::to_string(report).expect("input reports are serializable")
        )?;
    } else {
        let bytes = report
            .bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            output,
            "{:>10.6}s interface {}  {bytes}",
            report.elapsed_microseconds as f64 / 1_000_000.0,
            report.interface_number
        )?;
    }
    output.flush()?;
    Ok(())
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

    #[test]
    fn function_key_preset_has_documented_name() {
        let arguments =
            Arguments::try_parse_from(["darkmount", "assignments", "preset", "f13-f20"]).unwrap();
        assert!(matches!(
            arguments.command,
            Command::Assignments {
                command: Some(AssignmentCommand::Preset {
                    preset: AssignmentPreset::F13F20
                })
            }
        ));
    }

    #[test]
    fn missing_finite_choice_displays_its_options() {
        let error = match Arguments::try_parse_from(["darkmount", "assignments", "preset"]) {
            Err(error) => error,
            Ok(_) => panic!("missing preset was accepted"),
        };
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert!(
            error
                .to_string()
                .contains("<PRESET>  [possible values: f13-f20]")
        );
    }
}
