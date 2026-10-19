#!/usr/bin/env bash
# Extracts the base commit's committed reports/difftest into <out-dir> for the difftest
# ratchet and prints the argument to pass to difftest (`--baseline <dir>`) on stdout.
#
#   scripts/ci/difftest-baseline.sh <event> <base-sha> <out-dir>
#
# <event> is github.event_name, <base-sha> the PR base (pull_request) or the previous
# head (push). The ratchet may only be skipped where there is no base commit to compare
# with: a push whose `before` is empty or all zeros (a new branch), or a manual run.
# Then it prints nothing and exits 0. Anything else must produce a baseline: a pull
# request without a base, a failed fetch or extraction, or a base without any report
# exits nonzero, so CI never silently runs --green without its baseline.
# Tested by scripts/ci/test-difftest-baseline.sh.
set -euo pipefail
if [ $# -ne 3 ]; then
  echo "usage: $0 <event> <base-sha> <out-dir>" >&2
  exit 2
fi
event=$1 base=$2 out=$3
zero=0000000000000000000000000000000000000000

if [ -z "$base" ] || [ "$base" = "$zero" ]; then
  case "$event" in
    push | workflow_dispatch)
      echo "difftest-baseline: $event without a base commit; the ratchet is skipped" >&2
      exit 0
      ;;
    *)
      echo "difftest-baseline: $event needs a base commit, got '$base'" >&2
      exit 1
      ;;
  esac
fi

rm -rf "$out"
mkdir -p "$out"
if ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  if ! git fetch --quiet --depth 1 origin "$base"; then
    echo "difftest-baseline: cannot fetch base commit $base" >&2
    exit 1
  fi
fi
if ! git archive "$base" reports/difftest | tar -x -C "$out"; then
  echo "difftest-baseline: cannot extract reports/difftest from $base" >&2
  exit 1
fi
n=$(find "$out/reports/difftest" -maxdepth 1 -name '*.json' | wc -l | tr -d ' ')
if [ "$n" -eq 0 ]; then
  echo "difftest-baseline: $base has no reports in reports/difftest" >&2
  exit 1
fi
echo "difftest-baseline: $n reports from $base" >&2
echo "--baseline $out/reports/difftest"
