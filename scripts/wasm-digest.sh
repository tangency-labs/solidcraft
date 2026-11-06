#!/usr/bin/env bash
# Runs the Rust side of the goldens units natively and on wasm32-wasip1 (wasmtime) and
# compares the per-case output digests (`goldens --digest-dir`): equal digests mean
# bit-identical Rust outputs on both targets.
#
#   scripts/wasm-digest.sh [--jobs N] [--out DIR] [goldens selection...]   # default --all
#
# Needs `rustup target add wasm32-wasip1` and `wasmtime` on PATH. Writes DIR/{native,wasm}/<unit>.digest and
# DIR/mismatches.txt (default DIR: bin/wasm-digest); exits 1 on any mismatch.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/bin}
jobs=2
out=$root/bin/wasm-digest
sel=()
while [ $# -gt 0 ]; do
  case $1 in
    --jobs) jobs=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) sel+=("$1"); shift ;;
  esac
done
[ ${#sel[@]} -gt 0 ] || sel=(--all)
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -rf "$out/native" "$out/wasm" "$out/log"
mkdir -p "$out/native" "$out/wasm" "$out/log"

cd "$root"
cargo build --profile goldens -p goldens
cargo build --profile goldens -p goldens --target wasm32-wasip1
wasm=$target/wasm32-wasip1/goldens/goldens.wasm
wasmtime compile "$wasm" -o "$out/goldens.cwasm"

"$target/goldens/goldens" "${sel[@]}" --jobs "$jobs" --digest-dir "$out/native" >"$out/log/native.txt"
# one wasmtime process per unit: a trap or abort (panic = abort on wasm32) loses one unit
ls "$out/native" | sed -n 's/\.digest$//p' >"$out/units.txt"
# WASI does not follow a symlink out of a preopened dir: map a linked upstream/ itself
up=$(readlink -f "$root/upstream")
export out root up
xargs -P "$jobs" -I{} sh -c '
  wasmtime run --allow-precompiled --dir "$root::$root" --dir "$up::$root/upstream" \
    --dir "$out::$out" "$out/goldens.cwasm" \
    --unit {} --digest-dir "$out/wasm" >"$out/log/{}.txt" 2>&1 || echo "wasm exit $? {}" >>"$out/log/{}.txt"
' <"$out/units.txt"

: >"$out/mismatches.txt"
units=0 cases=0 bad=0 lost=0
while read -r id; do
  units=$((units + 1))
  w=$out/wasm/$id.digest
  if [ ! -f "$w" ]; then
    lost=$((lost + 1))
    echo "$id: no wasm digest ($(tail -n 1 "$out/log/$id.txt"))" >>"$out/mismatches.txt"
    continue
  fi
  read -r c m < <(paste "$out/native/$id.digest" "$w" | awk -F'\t' -v id="$id" -v f="$out/mismatches.txt" '
    { c++ }
    $3 != $1 { print id ": case order differs at line " NR >>f; m++; next }
    $2 != $4 { print id "\t" $1 "\tnative=" $2 "\twasm=" $4 >>f; m++ }
    END { print c + 0, m + 0 }')
  cases=$((cases + c)) bad=$((bad + m))
done <"$out/units.txt"
echo "wasm-digest: $units unit(s), $cases case(s), $bad mismatch(es), $lost unit(s) without wasm digest"
[ "$bad" -eq 0 ] && [ "$lost" -eq 0 ]
