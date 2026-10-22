#!/usr/bin/env bash
# Runs the Rust side of difftest units natively and on wasm32-wasip1 (wasmtime) and
# compares the per-case output digests (`difftest --digest-dir`): equal digests mean
# bit-identical Rust outputs on both targets. The C++ kernel is never run on wasm32: a
# case whose Rust side needs it (fixture exports) is `needs-cpp` there and skipped.
#
#   scripts/wasm-digest.sh [--jobs N] [--out DIR] [difftest selection...]   # default --green
#
# Needs `rustup target add wasm32-wasip1`, `wasmtime` on PATH and the C++ build
# (SOLIDCORE_CPP_DIR) for the native side. Writes DIR/{native,wasm}/<unit>.digest and
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
[ ${#sel[@]} -gt 0 ] || sel=(--green)
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -rf "$out/native" "$out/wasm" "$out/log"
mkdir -p "$out/native" "$out/wasm" "$out/log"

cd "$root"
cargo build --profile difftest -p difftest --features cpp
cargo build --profile difftest -p difftest --features cpp --target wasm32-wasip1
wasm=$target/wasm32-wasip1/difftest/difftest.wasm
python3 "$root/scripts/wasm_stub.py" "$wasm" "$out/stub.wat"
wasmtime compile "$wasm" -o "$out/difftest.cwasm"

"$target/difftest/difftest" "${sel[@]}" --jobs "$jobs" --digest-dir "$out/native" >"$out/log/native.txt"
# one wasmtime process per unit: a trap or abort (panic = abort on wasm32) loses one unit
ls "$out/native" | sed -n 's/\.digest$//p' >"$out/units.txt"
# WASI does not follow a symlink out of a preopened dir: map a linked upstream/ itself
up=$(readlink -f "$root/upstream")
export out root up
xargs -P "$jobs" -I{} sh -c '
  wasmtime run --allow-precompiled --dir "$root::$root" --dir "$up::$root/upstream" \
    --dir "$out::$out" --preload env="$out/stub.wat" "$out/difftest.cwasm" \
    --unit {} --digest-dir "$out/wasm" >"$out/log/{}.txt" 2>&1 || echo "wasm exit $? {}" >>"$out/log/{}.txt"
' <"$out/units.txt"

: >"$out/mismatches.txt"
units=0 cases=0 needs=0 bad=0 lost=0
while read -r id; do
  units=$((units + 1))
  w=$out/wasm/$id.digest
  if [ ! -f "$w" ]; then
    lost=$((lost + 1))
    echo "$id: no wasm digest ($(tail -n 1 "$out/log/$id.txt"))" >>"$out/mismatches.txt"
    continue
  fi
  read -r c n m < <(paste "$out/native/$id.digest" "$w" | awk -F'\t' -v id="$id" -v f="$out/mismatches.txt" '
    { c++ }
    $3 != $1 { print id ": case order differs at line " NR >>f; m++; next }
    $4 == "needs-cpp" { n++; next }
    $2 != $4 { print id "\t" $1 "\tnative=" $2 "\twasm=" $4 >>f; m++ }
    END { print c + 0, n + 0, m + 0 }')
  cases=$((cases + c)) needs=$((needs + n)) bad=$((bad + m))
done <"$out/units.txt"
echo "wasm-digest: $units unit(s), $cases case(s), $((cases - needs)) compared, $needs needs-cpp, $bad mismatch(es), $lost unit(s) without wasm digest"
[ "$bad" -eq 0 ] && [ "$lost" -eq 0 ]
