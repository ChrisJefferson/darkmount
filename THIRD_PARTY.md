# Third-party software

The Rust dependency graph recorded in `Cargo.lock` was audited when this
repository was created. Every dependency declares a permissive licence: MIT,
Apache-2.0, BSD-3-Clause, Zlib, Unlicense, Unicode-3.0, or a permitted
combination of those licences.

The direct dependencies are:

- `clap`: MIT OR Apache-2.0
- `serde` and `serde_json`: MIT OR Apache-2.0
- `hidapi`: MIT
- `chrono`: MIT OR Apache-2.0
- `fs2`: MIT/Apache-2.0
- `image`: MIT OR Apache-2.0

The `hidapi` crate builds the upstream HIDAPI C implementation on macOS and
Linux. Upstream HIDAPI offers a choice of GPL-3.0, BSD-3-Clause, or its original
permissive licence; darkmount uses it under the BSD-3-Clause option.

This file records the licensing decision but is not a generated bundle of all
dependency notices. Any future binary release must include the notices required
by the exact dependency versions in that release.
