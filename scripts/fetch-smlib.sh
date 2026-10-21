#!/usr/bin/env bash
# Fetches the SMLib sources solidcraft-smlib builds (feature `native`): tangency-labs/smlib at
# the commit in crates/smlib/SMLIB_REV, only the parts the build reads (source/SMLib,
# source/SM_API, capi), into target/smlib-<rev12>. OpenCASCADE is not part of the fork.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
rev="$(tr -d '[:space:]' < "$root/crates/smlib/SMLIB_REV")"
dest="${SOLIDCRAFT_SMLIB_DIR:-$root/target/smlib-${rev:0:12}}"
if [ -f "$dest/capi/include/smlib.h" ] && [ "$(git -C "$dest" rev-parse HEAD 2>/dev/null)" = "$rev" ]; then
  echo "SMLib $rev already at $dest"
  exit 0
fi
rm -rf "$dest"
mkdir -p "$dest"
git -C "$dest" init -q
git -C "$dest" remote add origin https://github.com/tangency-labs/smlib.git
git -C "$dest" sparse-checkout set --no-cone /source/SMLib/ /source/SM_API/ /capi/ /LICENSE
git -C "$dest" fetch -q --depth 1 --filter=blob:none origin "$rev"
git -C "$dest" checkout -q FETCH_HEAD
echo "SMLib $rev at $dest"
