#!/bin/sh
# Runs one unit's Rust side on wasm32-wasip1 for scripts/wasm-digest.sh.
#
# The caller passes the root, resolved upstream, output directory and unit id as
# arguments. This is a static file rather than a generated runner, and the id is
# never interpolated into shell code, so shell metacharacters stay data.
set -u
root=$1
up=$2
out=$3
unit=$4
wasmtime run --allow-precompiled --dir "$root::$root" --dir "$up::$root/upstream" \
  --dir "$out::$out" "$out/goldens.cwasm" \
  --unit "$unit" --digest-dir "$out/wasm" >"$out/log/$unit.txt" 2>&1 \
  || echo "wasm exit $? $unit" >>"$out/log/$unit.txt"
