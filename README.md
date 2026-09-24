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
darkmount assignments
darkmount assignments set-key 0x63 0x04 --modifier left-control
darkmount assignments default 0x63
darkmount assignments website display-1 https://www.bequiet.com/en
darkmount assignments preset f13-f20
darkmount snapshot snapshot-directory
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
darkmount lighting status
darkmount lighting mode general
darkmount lighting effect static --colour '#ff2800'
darkmount lighting effect colour-wave --direction left --colour '#ff2800'
darkmount game-mode settings
darkmount game-mode configure --windows-key block --alt-tab allow
darkmount events
darkmount trace-input --seconds 10
```

`--json` provides machine-readable output for identification, settings,
onboard assignments, LampArray information and key events. Use `--help` on a
command to see its validated options.

`darkmount trace-input` is a bounded, read-only diagnostic which records the
raw reports emitted by the keyboard's ordinary input interfaces. It never
sends a report to the keyboard. On macOS, the terminal application running it
must be allowed under System Settings → Privacy & Security → Input Monitoring.
Only type or press the keys relevant to the test while a trace is running,
because the output contains raw keyboard input.

`darkmount snapshot` creates a new directory containing `manifest.json`, all
eight exact stored JPEGs, upright PNG versions, the exact dock RGB565 pixels
and a viewable dock PNG. The manifest records device information, all current
onboard assignment entries, dock settings and the image filenames. Snapshot
is read-only and refuses to replace an existing directory.

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

Dock-setting writes, game-mode settings, assignment writes and onboard-lighting
mode changes are also read back and compared. The dock clock uses the
keyboard's unsolicited confirmation. Onboard lighting effects have no known
detailed read command, so changing an effect can check only the keyboard's
acknowledgement.

Assignment targets are `display-1` through `display-8`, or an explicit decimal
or hexadecimal device key ID. Application, website and Media “Next effect”
assignments are restricted to display keys because those are the forms present
in the captures. Default restoration is currently exposed only for F12
(`0x63`), whose resulting default action was captured and can therefore be
checked exactly.

### F13–F20 display-key preset

`darkmount assignments preset f13-f20` assigns display keys 1–8 to the
standard USB keyboard keys F13–F20 respectively. The command reads and verifies
every resulting assignment; rerunning it leaves assignments that are already
correct unchanged. These assignments replace the display keys' current onboard
actions.

| Display key | macOS key | USB HID keycode |
| ---: | ---: | ---: |
| 1 | F13 | `0x68` |
| 2 | F14 | `0x69` |
| 3 | F15 | `0x6a` |
| 4 | F16 | `0x6b` |
| 5 | F17 | `0x6c` |
| 6 | F18 | `0x6d` |
| 7 | F19 | `0x6e` |
| 8 | F20 | `0x6f` |

On macOS these keys can be attached to actions without a Dark Mount daemon. In
Shortcuts, open a shortcut's details, choose **Add Keyboard Shortcut**, then
press the corresponding display key. An unassigned F13–F20 key normally has no
visible effect. A listener using the vendor display-key events remains the
better option for dynamic or stateful actions.

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
