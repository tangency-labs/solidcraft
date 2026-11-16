#!/usr/bin/env bash
# Focused portability tests for scripts/wasm-digest.sh.
#
# The real script runs against a throwaway rig (a copy of the script, a symlinked
# upstream/ and fake cargo, wasmtime and goldens), so no wasm target,
# wasmtime installation or kernel compile is needed. The fakes record what the
# script asked them to do; the wasmtime fake also checks the WASI preopens.
#
# The pre-fix script fails here for two independent reasons:
#   * `readlink -f` is missing on older BSD/macOS (the fake below simulates that;
#     current macOS does support it);
#   * its `sh -c` runner puts `{}` inside shell code, so BSD xargs's 255-byte
#     substituted-argument limit rejects the whole template ("command line cannot
#     be assembled, too long").
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
script=$repo/scripts/wasm-digest.sh
tmp=$(mktemp -d "${TMPDIR:-/tmp}/wasm-digest-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

real_xargs=$(command -v xargs)
fail=0

ok() { printf 'ok   %s\n' "$1"; }
no() { printf 'FAIL %s: %s\n' "$1" "$2"; fail=1; }

# Exit status of a command run with output captured in $log.
run_digest() {
  local rig=$1 log=$2
  shift 2
  set +e
  CARGO_TARGET_DIR=$rig/target \
    FAKE_EXPECT_ROOT=$rig FAKE_EXPECT_UP=${RUN_EXPECT_UP:-$rig/real-upstream} \
    FAKE_EXPECT_OUT=${RUN_EXPECT_OUT:-$rig/out} \
    FAKE_REAL_XARGS=$real_xargs \
    PATH=$rig/fakebin:$PATH \
    "$rig/scripts/wasm-digest.sh" "$@" >"$log" 2>&1
  local status=$?
  set -e
  printf '%s' "$status"
}

# make_rig NAME: a rig with the real script, a symlinked upstream/ and fake tools.
# The rig path deliberately has no spaces unless the caller adds one.
make_rig() {
  local rig
  case $1 in
    /*) rig=$1 ;;
    *) rig=$tmp/$1 ;;
  esac
  mkdir -p "$rig/scripts" "$rig/fakebin" "$rig/real-upstream" \
    "$rig/target/goldens" "$rig/target/wasm32-wasip1/goldens"
  cp "$script" "$rig/scripts/wasm-digest.sh"
  if [ -f "$repo/scripts/wasm-digest-unit.sh" ]; then
    cp "$repo/scripts/wasm-digest-unit.sh" "$rig/scripts/wasm-digest-unit.sh"
  fi
  ln -s "$rig/real-upstream" "$rig/upstream"
  : >"$rig/target/wasm32-wasip1/goldens/goldens.wasm"

  cat >"$rig/fakebin/cargo" <<'SH'
#!/bin/sh
case "$*" in
  'build --profile goldens -p goldens'|'build --profile goldens -p goldens --target wasm32-wasip1') exit 0 ;;
  *) echo "fake cargo: unexpected invocation: $*" >&2; exit 2 ;;
esac
SH

  cat >"$rig/fakebin/goldens" <<'SH'
#!/bin/sh
set -e
digest=
units=
jobs=
while [ $# -gt 0 ]; do
  case $1 in
    --digest-dir) digest=$2; shift 2 ;;
    --jobs) jobs=$2; shift 2 ;;
    --unit) units="$units${units:+ }$2"; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$digest" ] || { echo "fake goldens: no --digest-dir" >&2; exit 2; }
mkdir -p "$digest"
printf '%s\n' "$units" >"$digest/../selected-units.txt"
printf '%s\n' "$jobs" >"$digest/../selected-jobs.txt"
for u in $units; do
  printf 'case-a\t100\ncase-b\t200\n' >"$digest/$u.digest"
done
SH

  cat >"$rig/fakebin/wasmtime" <<'SH'
#!/bin/sh
set -e
if [ "${FAKE_WASMTIME_LOG:-}" ]; then printf '%s\n' "$*" >>"$FAKE_WASMTIME_LOG"; fi
case ${1:-} in
  compile)
    shift
    [ "${1:-}" = "$CARGO_TARGET_DIR/wasm32-wasip1/goldens/goldens.wasm" ] && [ -f "$1" ] \
      || { echo "fake wasmtime: wrong goldens wasm input" >&2; exit 2; }
    out=
    while [ $# -gt 0 ]; do
      case $1 in -o) out=$2; shift 2 ;; *) shift ;; esac
    done
    [ -n "$out" ] || { echo "fake wasmtime compile: no -o" >&2; exit 2; }
    : >"$out"
    ;;
  run)
    shift
    [ "$PWD" = "$FAKE_EXPECT_ROOT" ] || { echo "fake wasmtime: cwd '$PWD'" >&2; exit 2; }
    digest= unit= root_dir= up_dir= out_dir= module=
    while [ $# -gt 0 ]; do
      case $1 in
        --digest-dir) digest=$2; shift 2 ;;
        --unit) unit=$2; shift 2 ;;
        --dir)
          case ${root_dir:-} in
            '') root_dir=$2 ;;
            *) [ -n "${up_dir:-}" ] && out_dir=$2 || up_dir=$2 ;;
          esac
          shift 2 ;;
        --preload) echo 'fake wasmtime: retired C++ preload is forbidden' >&2; exit 2 ;;
        *.cwasm) module=$1; shift ;;
        *) shift ;;
      esac
    done
    # The preopens must expose upstream/ by its real path (WASI will not follow a
    # symlink out of a preopened directory) and log/out by their configured paths.
    [ "${FAKE_EXPECT_ROOT:-}" = "" ] || [ "$root_dir" = "$FAKE_EXPECT_ROOT::$FAKE_EXPECT_ROOT" ] \
      || { echo "fake wasmtime: root dir '$root_dir'" >&2; exit 2; }
    [ "${FAKE_EXPECT_UP:-}" = "" ] || [ "$up_dir" = "$FAKE_EXPECT_UP::$FAKE_EXPECT_ROOT/upstream" ] \
      || { echo "fake wasmtime: upstream dir '$up_dir'" >&2; exit 2; }
    [ "${FAKE_EXPECT_OUT:-}" = "" ] || [ "$out_dir" = "$FAKE_EXPECT_OUT::$FAKE_EXPECT_OUT" ] \
      || { echo "fake wasmtime: out dir '$out_dir'" >&2; exit 2; }
    [ "$module" = "$FAKE_EXPECT_OUT/goldens.cwasm" ] && [ -f "$module" ] \
      || { echo "fake wasmtime: wrong goldens module '$module'" >&2; exit 2; }
    if [ "${FAKE_WASM_FAIL_UNIT:-}" = "$unit" ]; then
      echo "fake wasmtime: trap in $unit" >&2
      exit 7
    fi
    [ -n "$digest" ] && [ -n "$unit" ] || { echo "fake wasmtime: run without --unit/--digest-dir" >&2; exit 2; }
    [ -f "$FAKE_EXPECT_OUT/native/$unit.digest" ] || { echo "fake wasmtime: unexpected unit '$unit'" >&2; exit 2; }
    mkdir -p "$digest"
    if [ "${FAKE_WASM_DIFF_UNIT:-}" = "$unit" ]; then
      printf 'case-a\t999\ncase-b\t200\n' >"$digest/$unit.digest"
    else
      printf 'case-a\t100\ncase-b\t200\n' >"$digest/$unit.digest"
    fi
    ;;
  *) echo "fake wasmtime: unexpected invocation: $*" >&2; exit 2 ;;
esac
SH

  # GNU-style `readlink -f`: works only for a directory (the upstream symlink).
  cat >"$rig/fakebin/readlink-gnu" <<'SH'
#!/bin/sh
[ "${1:-}" = "-f" ] || { echo "fake readlink: only -f" >&2; exit 2; }
(cd "$2" && pwd -P)
SH

  # Simulated older BSD/macOS readlink: no -f at all (current macOS has it).
  cat >"$rig/fakebin/readlink-bsd" <<'SH'
#!/bin/sh
echo "readlink: illegal option -- ${1#-}" >&2
exit 1
SH

  # BSD xargs caps the argument(s) containing the replacement after substitution
  # (255 bytes by default); static argv, however long, is not bounded.
  cat >"$rig/fakebin/xargs-bsd" <<'SH'
#!/bin/sh
in=$(mktemp "${TMPDIR:-/tmp}/fake-xargs.XXXXXX")
cat >"$in"
max=0
while IFS= read -r line; do
  n=${#line}
  [ "$n" -gt "$max" ] && max=$n
done <"$in"
too_long=0
for a in "$@"; do
  case $a in
    *'{}'*)
      n=$((${#a} - 2 + max))
      [ "$n" -gt 255 ] && too_long=1
      ;;
  esac
done
if [ "$too_long" -eq 1 ]; then
  rm -f "$in"
  echo "xargs: command line cannot be assembled, too long" >&2
  exit 1
fi
"$FAKE_REAL_XARGS" "$@" <"$in"
rc=$?
rm -f "$in"
exit $rc
SH

  chmod +x "$rig/fakebin/"*
  cp "$rig/fakebin/goldens" "$rig/target/goldens/goldens"
  chmod +x "$rig/target/goldens/goldens"
  # Canonical path: the script resolves root with `cd`/`pwd`, so a trailing `//`
  # from TMPDIR would otherwise disagree with the fake tools.
  (cd "$rig" && pwd -P)
}

# select_gnu_readlink RIG: GNU-like readlink, so the xargs defect is reachable.
select_gnu_readlink() {
  cp "$1/fakebin/readlink-gnu" "$1/fakebin/readlink"
}

# select_bsd_tools RIG: simulated older-BSD readlink plus the BSD xargs cap.
select_bsd_tools() {
  cp "$1/fakebin/readlink-bsd" "$1/fakebin/readlink"
  cp "$1/fakebin/xargs-bsd" "$1/fakebin/xargs"
  chmod +x "$1/fakebin/readlink" "$1/fakebin/xargs"
}

# --- 1. older-BSD readlink (no -f) -----------------------------------------
rig=$(make_rig bsd-readlink)
select_bsd_tools "$rig"
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit unit.one --unit unit.two)
if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 2 unit(s)' "$rig/run.log"; then
  ok 'older BSD readlink: upstream resolves without readlink -f'
else
  no 'older BSD readlink: upstream resolves without readlink -f' \
    "exit $status: $(grep -m1 'readlink\|xargs\|error' "$rig/run.log" || tail -n 1 "$rig/run.log")"
fi

# --- 2. BSD xargs template cap, with readlink out of the way ---------------
rig=$(make_rig bsd-xargs)
select_gnu_readlink "$rig"
cp "$rig/fakebin/xargs-bsd" "$rig/fakebin/xargs"
chmod +x "$rig/fakebin/xargs"
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit unit.one --unit unit.two)
if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 2 unit(s), 4 case(s), 0 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'BSD xargs: per-unit runner fits the 255-byte template'
else
  no 'BSD xargs: per-unit runner fits the 255-byte template' \
    "exit $status: $(grep -m1 'cannot be assembled\|xargs' "$rig/run.log" || tail -n 1 "$rig/run.log")"
fi

# --- 3. long TMPDIR with the host's real xargs -----------------------------
# Static argv may be long; only a replaced argument is capped. The real macOS
# xargs must accept the static helper path even when it is far past 255 bytes.
long=$tmp
for _ in 1 2 3; do
  long=$long/$(printf '%100s' '' | tr ' ' x)
done
rig=$(make_rig "$long/long-tmpdir")
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit unit.one --unit unit.two)
if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 2 unit(s), 4 case(s), 0 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'long TMPDIR: real xargs accepts a long static helper path'
else
  no 'long TMPDIR: real xargs accepts a long static helper path' \
    "exit $status: $(grep -m1 'xargs\|error' "$rig/run.log" || tail -n 1 "$rig/run.log")"
fi

# --- 4. paths with spaces, symlinked upstream, BSD tools -------------------
rig=$(make_rig 'space rig')
select_bsd_tools "$rig"
if [ -d "$rig/upstream" ] && [ -L "$rig/upstream" ]; then
  export RUN_EXPECT_OUT="$rig/out dir/with space"
  status=$(run_digest "$rig" "$rig/run.log" --jobs 2 --out "$rig/out dir/with space" \
    --unit unit.one --unit unit.two)
  unset RUN_EXPECT_OUT
  if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 2 unit(s)' "$rig/run.log"; then
    ok 'paths with spaces + symlinked upstream: preopens and logs stay quoted'
  else
    no 'paths with spaces + symlinked upstream: preopens and logs stay quoted' \
      "exit $status: $(grep -m1 'space\|xargs\|readlink\|fake wasmtime' "$rig/run.log" || tail -n 1 "$rig/run.log")"
  fi
else
  no 'paths with spaces + symlinked upstream: preopens and logs stay quoted' 'rig setup failed'
fi

# --- 5. selection, jobs and multi-unit runs --------------------------------
rig=$(make_rig selection)
select_gnu_readlink "$rig"
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --jobs 3 \
  --unit unit.one --unit unit.two --unit unit.three)
if [ "$status" -eq 0 ] \
  && [ "$(cat "$rig/out/selected-units.txt")" = 'unit.one unit.two unit.three' ] \
  && [ "$(cat "$rig/out/selected-jobs.txt")" = '3' ] \
  && grep -q 'wasm-digest: 3 unit(s), 6 case(s), 0 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'selection: every --unit reaches goldens and every unit runs on wasm'
else
  no 'selection: every --unit reaches goldens and every unit runs on wasm' \
    "exit $status: units='$(cat "$rig/out/selected-units.txt" 2>/dev/null || echo -)' jobs='$(cat "$rig/out/selected-jobs.txt" 2>/dev/null || echo -)' $(tail -n 1 "$rig/run.log")"
fi

# --- 6. a trapped wasm unit is reported and fails the run ------------------
rig=$(make_rig trap)
select_gnu_readlink "$rig"
export FAKE_WASM_FAIL_UNIT=unit.two
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit unit.one --unit unit.two)
unset FAKE_WASM_FAIL_UNIT
if [ "$status" -eq 1 ] \
  && grep -q '^unit.two: no wasm digest (wasm exit 7 unit.two)$' "$rig/out/mismatches.txt" \
  && grep -q 'wasm-digest: 2 unit(s), .*0 mismatch(es), 1 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'failure: a trapped unit is logged, listed and fails the run'
else
  no 'failure: a trapped unit is logged, listed and fails the run' \
    "exit $status, mismatches: $(cat "$rig/out/mismatches.txt" 2>/dev/null || echo -) $(tail -n 1 "$rig/run.log")"
fi

# --- 7. no runner file is written into the output directory -----------------
rig=$(make_rig clobber)
select_gnu_readlink "$rig"
mkdir -p "$rig/out"
printf 'keep me\n' >"$rig/victim"
ln -s "$rig/victim" "$rig/out/run-unit.sh"
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit unit.one --unit unit.two)
if [ "$status" -eq 0 ] && [ "$(cat "$rig/victim")" = 'keep me' ]; then
  ok 'output dir: runner is not written or executed (symlink not clobbered)'
else
  no 'output dir: runner is not written or executed (symlink not clobbered)' \
    "exit $status, victim='$(cat "$rig/victim" 2>/dev/null || echo -)' $(tail -n 1 "$rig/run.log")"
fi

# --- 8. literal shell metacharacters in a unit id --------------------------
rig=$(make_rig metachar)
select_gnu_readlink "$rig"
id="unit;touch\${IFS}PWNED"
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out" --unit "$id")
if [ "$status" -eq 0 ] && [ -f "$rig/out/wasm/$id.digest" ] \
  && [ ! -e "$rig/PWNED" ] && [ ! -e "$PWD/PWNED" ]; then
  ok 'unit ids with shell metacharacters stay literal data'
else
  no 'unit ids with shell metacharacters stay literal data' \
    "exit $status, digest=$([ -f "$rig/out/wasm/$id.digest" ] && echo yes || echo no), pwned=$([ -e "$rig/PWNED" ] && echo yes || echo no) $(tail -n 1 "$rig/run.log")"
fi

# --- 9. a differing digest is reported and fails the run -------------------
rig=$(make_rig mismatch)
select_gnu_readlink "$rig"
set +e
FAKE_WASM_DIFF_UNIT=unit.one \
  CARGO_TARGET_DIR=$rig/target \
  FAKE_EXPECT_ROOT=$rig FAKE_EXPECT_UP=$rig/real-upstream FAKE_EXPECT_OUT=$rig/out \
  FAKE_REAL_XARGS=$real_xargs \
  PATH=$rig/fakebin:$PATH \
  "$rig/scripts/wasm-digest.sh" --out "$rig/out" --unit unit.one --unit unit.two >"$rig/run.log" 2>&1
status=$?
set -e
if [ "$status" -eq 1 ] \
  && awk -F '\t' 'NR == 1 && $1 == "unit.one" && $2 == "case-a" && $3 == "native=100" && $4 == "wasm=999" { found = 1 } END { exit !found }' "$rig/out/mismatches.txt" \
  && grep -q 'wasm-digest: 2 unit(s), .*1 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'failure: a differing case is listed with both digests and fails the run'
else
  no 'failure: a differing case is listed with both digests and fails the run' \
    "exit $status, mismatches: $(cat "$rig/out/mismatches.txt" 2>/dev/null || echo -) $(tail -n 1 "$rig/run.log")"
fi

# --- 10. literal xargs replacement markers in static paths -----------------
for variant in root upstream out combined; do
  case $variant in
    root|combined) rig=$(make_rig "brace-{}-$variant") ;;
    *) rig=$(make_rig "brace-$variant") ;;
  esac
  up=$rig/real-upstream
  out=$rig/out
  case $variant in
    upstream|combined)
      up=$rig/'real-{}-upstream'
      mkdir -p "$up"
      unlink "$rig/upstream"
      ln -s "$up" "$rig/upstream" ;;
  esac
  case $variant in out|combined) out=$rig/'out-{}' ;; esac
  export RUN_EXPECT_UP=$up RUN_EXPECT_OUT=$out
  status=$(run_digest "$rig" "$rig/run.log" --jobs 2 --out "$out" --unit unit.one --unit unit.two)
  unset RUN_EXPECT_UP RUN_EXPECT_OUT
  if [ "$status" -eq 0 ] \
    && grep -q 'wasm-digest: 2 unit(s), 4 case(s), 0 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log" \
    && [ -f "$out/wasm/unit.one.digest" ] && [ -f "$out/wasm/unit.two.digest" ]; then
    ok "literal braces: $variant paths and unit reach the helper unchanged"
  else
    no "literal braces: $variant paths and unit reach the helper unchanged" \
      "exit $status: $(tail -n 1 "$rig/run.log") $(cat "$out/log/"*.txt 2>/dev/null || :)"
  fi
done

# Static paths containing shell syntax must remain data, not shell code.
rig=$(make_rig 'quote'"'"'" dollar$ backtick` semicolon;')
out=$rig/'out-{}-"-$-`-;'
export RUN_EXPECT_OUT=$out
status=$(run_digest "$rig" "$rig/run.log" --jobs 2 --out "$out" --unit unit.one --unit unit.two)
unset RUN_EXPECT_OUT
if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 2 unit(s), 4 case(s), 0 mismatch(es), 0 unit(s) without wasm digest' "$rig/run.log"; then
  ok 'shell metacharacters in static paths stay literal data'
else
  no 'shell metacharacters in static paths stay literal data' "exit $status: $(tail -n 1 "$rig/run.log")"
fi

# --- 11. empty selection must not launch a unit helper ----------------------
rig=$(make_rig empty)
cat >"$rig/fakebin/xargs" <<'SH'
#!/bin/sh
echo 'xargs must not run for an empty selection' >&2
exit 90
SH
chmod +x "$rig/fakebin/xargs"
export FAKE_WASMTIME_LOG=$rig/wasmtime.log
status=$(run_digest "$rig" "$rig/run.log" --out "$rig/out")
unset FAKE_WASMTIME_LOG
if [ "$status" -eq 0 ] && grep -q 'wasm-digest: 0 unit(s), 0 case(s)' "$rig/run.log" \
  && ! grep -q '^run ' "$rig/wasmtime.log" && [ ! -s "$rig/out/units.txt" ]; then
  ok 'empty selection: no unit helper runs'
else
  no 'empty selection: no unit helper runs' "exit $status: $(tail -n 1 "$rig/run.log")"
fi

if [ "$fail" -ne 0 ]; then
  echo 'wasm-digest portability tests failed (see the reasons above)' >&2
  exit 1
fi
echo 'wasm-digest portability tests passed'
