// SPDX-License-Identifier: MPL-2.0

use clap::{Parser, Subcommand};
use darkmount::device;
use darkmount::support::{FirmwareVersion, control_support};

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
    /// Report whether a firmware version is supported for control operations.
    Support { version: FirmwareVersion },
}

fn main() {
    if let Err(error) = run(Arguments::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    match arguments.command {
        Command::Devices => {
            let devices = device::enumerate()?;
            if arguments.json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
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
        }
        Command::Support { version } => {
            let support = control_support(version);
            if arguments.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "firmware": version,
                        "control_support": support,
                    })
                );
            } else {
                println!("firmware {version}: {support:?}");
            }
        }
    }
    Ok(())
}
