#!/usr/bin/env bash
# Build local probes against the pinned original SMLib and compare their outputs.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CXX="${CXX:-g++}"
TARGET="${CARGO_TARGET_DIR:-$ROOT/bin}"
CPP_DIR="${SOLIDCORE_CPP_DIR:-$ROOT/bin/cpp}"
EVIDENCE="${ADAPTER_EVIDENCE_DIR:-$TARGET/adapter-evidence}"
MODE="${1:-all}"
case "$MODE" in wire|merge|all) ;; *) echo 'Expected wire, merge or all' >&2; exit 2 ;; esac
mkdir -p "$EVIDENCE"
if [ ! -f "$CPP_DIR/lib/libSMLib.a" ]; then
  SOLIDCORE_CPP_DIR="$CPP_DIR" nice -n 10 harness/cpp/build.sh --no-tests --jobs "$CARGO_BUILD_JOBS"
fi
CPP_FLAGS=(-std=c++17 -O2 -ffp-contract=off -fno-fast-math -w -DNDEBUG -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -D_GLIBCXX_USE_CXX11_ABI=1)
if [ "$MODE" = wire ] || [ "$MODE" = all ]; then
  nice -n 10 cargo build --locked -p solidcore --example wire_sweep
  nice -n 10 "$CXX" "${CPP_FLAGS[@]}" -I upstream/source/SMLib/inc harness/cpp/wire_sweep_reference.cpp "$CPP_DIR/lib/libSMLib.a" -pthread -o "$EVIDENCE/wire-sweep-cpp"
  python3 scripts/check-production-wire.py --cxx "$CXX" --rust "$TARGET/debug/examples/wire_sweep" --cpp "$EVIDENCE/wire-sweep-cpp" --cpp-dir "$CPP_DIR" --output-dir "$EVIDENCE"
fi
if [ "$MODE" = merge ] || [ "$MODE" = all ]; then
  nice -n 10 cargo build --locked -p solidcore --example merge_adapter
  nice -n 10 "$CXX" "${CPP_FLAGS[@]}" -I upstream/source/SMLib/inc harness/cpp/merge_adapter_reference.cpp "$CPP_DIR/lib/libSMLib.a" -pthread -o "$EVIDENCE/merge-adapter-cpp"
  python3 scripts/check-production-merge.py --rust "$TARGET/debug/examples/merge_adapter" --cpp "$EVIDENCE/merge-adapter-cpp" --cpp-library "$CPP_DIR/lib/libSMLib.a" --output "$EVIDENCE/merge-parity.json"
fi
