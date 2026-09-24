# macOS hardware release test

Run this checklist from a normal Terminal application on an otherwise idle Mac.
Disconnect IO Center Web and close any other program controlling the keyboard
before starting. The supported test device is model 1, hardware revision 1,
with all three firmware components at exactly 1.29.0.

## Read-only regression

Build the exact release source and run the bounded read-only script:

```sh
cargo build --release --locked
scripts/macos-read-only-smoke.sh
```

The script must complete without errors and create a new snapshot directory.
Inspect `manifest.json`, open the eight upright display-key PNGs and the dock
PNG, and confirm that none is blank, truncated or incorrectly oriented.

## Failure-path cleanup

Run an operation which opens a QLink session but fails validation before it can
write, followed immediately by a normal identification request:

```sh
target/release/darkmount lighting effect static --brightness 0
target/release/darkmount info
```

The first command must reject the brightness. The second must connect normally,
showing that the failed command did not leave a stale session behind.

Then exercise the LampArray guard's error path:

```sh
target/release/darkmount lamp-array solid not-a-colour --seconds 2
```

The command must reject the colour and the keyboard must remain under its
normal onboard lighting control. If it does not, run
`target/release/darkmount lamp-array release` and record the failure.

## Display-key input

Confirm that `darkmount assignments` reports display keys 1–8 as F13–F20. If
the preset has not yet been applied, apply and verify it:

```sh
target/release/darkmount assignments preset f13-f20
```

Give Terminal Input Monitoring permission, then capture one press and release
of each display key in order:

```sh
target/release/darkmount --json trace-input --seconds 15 > /tmp/darkmount-f13-f20.jsonl
```

The capture must contain keyboard usage IDs 104 through 111 (F13 through F20)
exactly once each, with a corresponding all-zero release report and no
unexpected modifiers.

## Transient lighting

Run:

```sh
target/release/darkmount lamp-array solid '#ff2800' --seconds 2
```

Confirm that every lamp becomes orange and that control returns to the
keyboard's onboard lighting after two seconds. Run
`target/release/darkmount lamp-array release` if it does not.

## Persistence and reconnect

Unplug and reconnect the keyboard. Confirm that:

- it still functions as a keyboard;
- `darkmount info` still reports firmware 1.29.0;
- `darkmount assignments` still reports F13–F20;
- the display and dock images still read successfully;
- onboard lighting is again under keyboard control.

Record the source commit, macOS version, Mac architecture, keyboard serial
number and the result of each section. A compiled binary is not considered
hardware-tested merely because these unit tests pass in CI.
