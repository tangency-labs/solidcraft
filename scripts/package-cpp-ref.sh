#!/usr/bin/env bash
# Package a finished harness/cpp/build.sh output for scripts/fetch-cpp.sh.
#
# Usage: scripts/package-cpp-ref.sh <cpp dir> <out dir>
#
# Writes <out dir>/solidcore-cpp-ref-<key>-linux-x86_64.tar.zst and a matching .sha256
# (sha256sum format), with <key> from scripts/cpp-ref-key.sh. The archive holds
# lib/libSMLib.a, lib/libSM_API.a, ffi.env, LICENSE (upstream Apache-2.0), NOTICE and
# BUILDINFO. ffi.env is relocatable as is: its paths are relative to the repository root
# (upstream/source/*/inc), which solidcore-ffi/build.rs resolves, and it has no absolute
# paths. compile_commands.json and the test apps are left out (absolute paths, not
# needed by solidcore-ffi). The libraries keep their -g debug info, as a source build.
# Run by .github/workflows/cpp-reference.yml.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CPP="${1:?usage: package-cpp-ref.sh <cpp dir> <out dir>}"
OUTDIR="${2:?usage: package-cpp-ref.sh <cpp dir> <out dir>}"
KEY="$("$ROOT/scripts/cpp-ref-key.sh")"
COMMIT="${UPSTREAM_COMMIT:-$(sed -n 's/^UPSTREAM_COMMIT=//p' "$ROOT/scripts/upstream-pin.env")}"
NAME="solidcore-cpp-ref-$KEY-linux-x86_64.tar.zst"

for f in lib/libSMLib.a lib/libSM_API.a ffi.env; do
  [ -f "$CPP/$f" ] || { echo "missing $CPP/$f (run harness/cpp/build.sh first)" >&2; exit 1; }
done
if sed 's/#.*//' "$CPP/ffi.env" | grep -Eq '(^|[ =])/'; then
  echo "$CPP/ffi.env has absolute paths; it would not be relocatable" >&2
  exit 1
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/lib" "$OUTDIR"
cp "$CPP/lib/libSMLib.a" "$CPP/lib/libSM_API.a" "$STAGE/lib/"
cp "$CPP/ffi.env" "$STAGE/ffi.env"
cp "$ROOT/upstream/LICENSE" "$STAGE/LICENSE"
cat > "$STAGE/NOTICE" <<NOTICE
solidcore C++ reference build of SMLib and SM_API

These static libraries are compiled, unmodified, from the SMLib and SM_API
components of USD BRep, https://github.com/NVIDIA-Omniverse/usd-brep
(commit $COMMIT), by harness/cpp/build.sh of
https://github.com/tangency-labs/solidcore. They exist only to run solidcore's
difftest against the original kernel.

  USD BRep is Copyright (c) 2020-2026, NVIDIA CORPORATION & AFFILIATES. All
  rights reserved. NVIDIA-authored portions of this product are licensed under
  the Apache License, Version 2.0, except where otherwise noted.

  SPDX-License-Identifier: Apache-2.0

The licence text is in LICENSE.
NOTICE
{
  echo "key=$KEY"
  echo "upstream_commit=$COMMIT"
  echo "solidcore_commit=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  # shellcheck disable=SC1091
  echo "os=$( (. /etc/os-release && echo "$PRETTY_NAME") 2>/dev/null || uname -s)"
  echo "arch=$(uname -m)"
  echo "glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null || echo unknown)"
  echo "cxx=$("${CXX:-g++}" --version 2>/dev/null | head -1)"
} > "$STAGE/BUILDINFO"
cat "$STAGE/BUILDINFO"

# Fixed order, owner and mtime so the tarball only depends on its contents.
( cd "$STAGE" && tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 \
    -cf - lib ffi.env LICENSE NOTICE BUILDINFO ) \
  | zstd -q -T0 -19 -o "$OUTDIR/$NAME" -f
( cd "$OUTDIR" && sha256sum "$NAME" > "$NAME.sha256" )
ls -l "$OUTDIR/$NAME"
cat "$OUTDIR/$NAME.sha256"
