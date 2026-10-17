#!/usr/bin/env bash
# Self-hosted runner only: keep the Cargo build directory between jobs, outside the checkout
# (actions/checkout's clean deletes bin/), and point CARGO_TARGET_DIR at it for the rest of
# the job. Each self-hosted runner has its own /home/runner and runs one job at a time, so the
# jobs on one runner share one directory and two runners never share one.
set -euo pipefail
dir=/home/runner/cargo-target/solidcore
# Start over when it has grown past 30 GB: old dependency versions and profiles pile up.
if [ -d "$dir" ] && [ "$(du -sm "$dir" | cut -f1)" -gt 30000 ]; then
  echo "warm-target: $dir is over 30 GB, starting a fresh build"
  rm -rf "$dir"
fi
mkdir -p "$dir"
echo "CARGO_TARGET_DIR=$dir" >> "$GITHUB_ENV"
echo "warm-target: $dir ($(du -sh "$dir" | cut -f1))"
