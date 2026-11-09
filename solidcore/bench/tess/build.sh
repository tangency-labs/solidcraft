#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build the harness (meshbake, mkprims) in release against the SolidCraft tree it sits in, with
# that tree's Cargo.lock and its own target folder (never shared with another tree: a shared one
# builds stale code). Prints the folder the binaries are in.
#
# Usage: bench/tess/build.sh            (inside a SolidCraft tree; to bench another commit, check it
#                                        out as a worktree and copy bench/tess/ into it first)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$(cd "$HERE/../../.." && pwd)"
cp "$APP/Cargo.lock" "$HERE/Cargo.lock"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$APP/target}"
cargo build --release --manifest-path "$HERE/Cargo.toml" >&2
echo "$CARGO_TARGET_DIR/release"
