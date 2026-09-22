# darkmount

`darkmount` is a headless Rust library for controlling the be quiet! Dark
Mount keyboard. It also builds one command-line executable, `darkmount`, for use
by people, scripts and programs written in other languages.

This is an independent community project. It is not affiliated with or
supported by be quiet!.

## Support contract

- Device: Dark Mount, USB `373f:0001`, model 1, hardware revision 1.
- Firmware: exactly 1.29.0 for control operations.
- Unknown firmware: enumeration and identification only; persistent writes are
  refused until that version is tested and explicitly supported.
- macOS on Apple Silicon: active development and hardware-tested.
- Linux: intended through HIDAPI's statically compiled hidraw backend, but not
  yet tested on hardware. A narrowly scoped udev rule will still be required.
- Firmware update, DFU and Dorkmount patch installation: deliberately absent.

The exact-version rule is intentional. The official update from 1.4.0 to
1.29.0 changed the display-key representation from 140×140 to 120×120.
Version ordering is therefore not evidence of protocol compatibility.

## Commands

There is one executable with subcommands:

```text
darkmount devices
darkmount info
darkmount display-key read 1 key-1.png
darkmount display-key write 1 image.jpg
darkmount dock settings
darkmount dock read-image dock.png
darkmount dock write-image image.png
darkmount dock configure --display image --idle-seconds 30
darkmount dock set-time
darkmount lamp-array info
darkmount lamp-array lamps
darkmount lamp-array solid '#ff2800' --seconds 5
darkmount lighting static --colour '#ff2800'
darkmount events
```

`--json` provides machine-readable output for identification, settings,
LampArray information and key events. Use `--help` on a command to see its
validated options.

Display-key images are presented upright by default. `--raw` reads or writes
the exact rotated JPEG stored by the keyboard. Dock images are ordinary PNGs
by default; their `--raw` form is exactly 153,600 bytes of little-endian
RGB565 pixels.

## Write policy

Image writes first read the currently stored bytes. If they are identical,
the command reports `nothing written` and sends no write command. An actual
write is divided into bounded protocol chunks and followed by a complete
readback which must be byte-identical.

There is no automatic backup directory and no persistent cooldown database.
Use `--backup PATH` when a particular replacement warrants a backup. Reads do
not wear flash, so verification is always enabled and has no bypass option.

A short-lived operating-system lock prevents two darkmount processes from
interleaving vendor-protocol traffic. It is released automatically when the
process exits; no daemon is involved.

Dock-setting writes are also read back and compared. The dock clock uses the
keyboard's unsolicited confirmation. Onboard lighting effects have no known
read command, so that operation can check only the keyboard's acknowledgement.

LampArray control is transient and standards-based. The CLI always returns
control to the keyboard after the requested holding time. Library callers
receive a control guard which must be released explicitly.

## Architecture

The crate separates HID transport, QLink framing/session management, image
conversion and device capabilities. Capability modules expose fixed validated
operations; the public API does not provide an unrestricted raw-command escape
hatch. Display-key events are exposed as data: launching commands, synthesising
keys and installing an autostart service belong in separate programs.

The initial transport uses the `hidapi` crate. On macOS this uses
IOHIDManager with shared device access. On Linux it builds the hidraw backend
into the executable.

## Provenance and licence

Protocol facts were documented by `re133/iocenter-linux`, inspected at commit
`6e7a10a27fe5d2e552dec9d7c6adb0ba17191da9`, and independently checked against
a real keyboard with a small native IOHIDManager probe. No source code was
copied or translated. See `ATTRIBUTION.md`.

MPL-2.0.
