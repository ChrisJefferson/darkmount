# Releasing darkmount

The initial distribution is source-only. Users install from GitHub or
crates.io with Cargo; there are no downloadable binaries to sign or notarise.

## Release scope

- macOS on Apple Silicon is hardware-tested.
- macOS on Intel is compile- and unit-tested, but not hardware-tested.
- Linux is not yet supported as a tested release platform.
- Control operations support only Dark Mount model 1, hardware revision 1,
  with all three firmware components at exactly 1.29.0.

## Checklist

1. Choose the version and replace `Unreleased` in `CHANGELOG.md` with the
   version and release date.
2. Run `cargo fmt --check`, `cargo test --locked`,
   `cargo clippy --locked --all-targets -- -D warnings`, and
   `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`.
3. Run the complete procedure in `docs/macos-hardware-test.md` against the
   exact release source.
4. Run `cargo package --locked` and `cargo publish --dry-run --locked` from a
   clean working tree. Inspect `cargo package --list` if the packaged files
   have changed.
5. Commit and push the release commit, then wait for both macOS CI jobs to
   pass.
6. Run `cargo publish --locked`. Publishing is irreversible, so do not use
   `--allow-dirty` for a real release.
7. Confirm that `cargo install darkmount --version VERSION --locked` succeeds.
8. Create an annotated `vVERSION` tag at the published commit and push it.

Do not claim Intel hardware support, Linux support, or compatibility with a
different firmware version solely because a build succeeds.
