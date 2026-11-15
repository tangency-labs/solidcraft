#!/usr/bin/env bash
# Bit-compares the std f64 functions solidcore calls on this host, wasm32-unknown-unknown
# (run under node) and, with docker, glibc on linux/amd64 and linux/arm64. See README.md.
# Work files go to bin/mathprobe/ (gitignored). Override CARGO/RUSTC to pick a toolchain.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$root/bin/mathprobe
mkdir -p "$work" && cd "$work"
cargo=${CARGO:-cargo}
export CARGO_TARGET_DIR=$work/target CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}

python3 -I "$here/gen.py"
(cd "$here" && nice -n 10 "$cargo" build -q --release --bin native)
(cd "$here" && nice -n 10 "$cargo" build -q --release --lib --target wasm32-unknown-unknown)
"$work/target/release/native" out/rust-host
python3 -I "$here/pyeval.py" out/py-host
node "$here/wasmeval.mjs" out/rust-wasm32 "$work/target/wasm32-unknown-unknown/release/mathprobe.wasm"
if command -v docker >/dev/null; then
  for arch in amd64 arm64; do
    docker run --rm --platform "linux/$arch" -v "$work:/w" -v "$here:/p:ro" -w /w ubuntu:24.04 bash -c \
      'apt-get update -qq >/dev/null && apt-get install -y -qq python3-minimal >/dev/null 2>&1 \
       && grep -o -w -E "fma|avx2" /proc/cpuinfo | sort -u | tr "\n" " "; python3 -I /p/pyeval.py out/py-glibc-'"$arch"
  done
fi
python3 -I "$here/compare.py"
