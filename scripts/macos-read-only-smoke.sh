#!/bin/sh
set -eu

binary=${1:-target/release/darkmount}
snapshot=${2:-${TMPDIR:-/tmp}/darkmount-smoke-$(date +%Y%m%d-%H%M%S)}

if [ ! -x "$binary" ]; then
    echo "error: release executable not found: $binary" >&2
    exit 1
fi

"$binary" --version
"$binary" devices
"$binary" info
"$binary" assignments
"$binary" dock settings
"$binary" lighting status
"$binary" game-mode settings
"$binary" lamp-array info
"$binary" snapshot "$snapshot"

echo "read-only smoke test complete: $snapshot"
