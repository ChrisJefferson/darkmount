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

## Command shape

There will be one executable with subcommands:

```text
darkmount devices
darkmount info
darkmount display-key read 1 key-1.jpg
darkmount display-key write 1 image.jpg
darkmount dock ...
darkmount lighting ...
```

Only `devices` and the development `support` query exist in the initial
scaffold. `--json` provides stable machine-readable output for other programs.

## Architecture

The crate separates HID transport, QLink framing/session management and device
capabilities. Capability modules expose fixed validated operations; the public
API will not provide an unrestricted raw-command escape hatch.

The initial transport uses the `hidapi` crate. On macOS this uses
IOHIDManager with shared device access. On Linux it builds the hidraw backend
into the executable.

## Provenance and licence

Protocol facts were documented by `re133/iocenter-linux`, inspected at commit
`6e7a10a27fe5d2e552dec9d7c6adb0ba17191da9`, and independently checked against
a real keyboard with a small native IOHIDManager probe. No source code was
copied or translated. See `ATTRIBUTION.md`.

MPL-2.0.
