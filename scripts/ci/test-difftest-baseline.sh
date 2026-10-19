#!/usr/bin/env bash
# Tests scripts/ci/difftest-baseline.sh in a throwaway repository (no network): a real base
# must yield a baseline, and every way of not getting one, except the documented bootstrap
# cases, must fail.
set -euo pipefail
script=$(cd "$(dirname "$0")" && pwd)/difftest-baseline.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fails=0

cd "$tmp"
git init -q repo && cd repo
git config user.email ci@example.invalid && git config user.name ci
# `origin` points nowhere, so any fetch fails.
git remote add origin "$tmp/no-such-remote"
echo a > README && git add README && git commit -q -m "no reports"
no_reports=$(git rev-parse HEAD)
mkdir -p reports/difftest && echo '{"unit":"a.b","cases":[]}' > reports/difftest/a.b.json
git add reports && git commit -q -m "reports"
with_reports=$(git rev-parse HEAD)
zero=0000000000000000000000000000000000000000

# expect <name> <exit status: 0|nonzero> <stdout: baseline|empty> <event> <base>
expect() {
  local name=$1 want=$2 out=$3 status=0 got
  got=$("$script" "$4" "$5" "$tmp/out" 2>"$tmp/err") || status=$?
  local ok=1
  if [ "$want" = 0 ] && [ "$status" -ne 0 ]; then ok=0; fi
  if [ "$want" = nonzero ] && [ "$status" -eq 0 ]; then ok=0; fi
  if [ "$out" = baseline ] && [ "$got" != "--baseline $tmp/out/reports/difftest" ]; then ok=0; fi
  if [ "$out" = empty ] && [ -n "$got" ]; then ok=0; fi
  if [ "$ok" = 1 ]; then
    echo "ok   $name"
  else
    echo "FAIL $name: status=$status stdout='$got' stderr='$(cat "$tmp/err")'"
    fails=$((fails + 1))
  fi
}

expect "pull request with a real base"        0       baseline pull_request "$with_reports"
expect "push with a real base"                0       baseline push "$with_reports"
expect "pull request, base fetch fails"       nonzero empty    pull_request 1234567890abcdef1234567890abcdef12345678
expect "push, base fetch fails"               nonzero empty    push 1234567890abcdef1234567890abcdef12345678
expect "pull request, base has no reports"    nonzero empty    pull_request "$no_reports"
expect "pull request without a base"          nonzero empty    pull_request ""
expect "pull request with an all-zero base"   nonzero empty    pull_request "$zero"
expect "push creating a branch (bootstrap)"   0       empty    push "$zero"
expect "manual run (bootstrap)"               0       empty    workflow_dispatch ""
expect "unknown event without a base"         nonzero empty    schedule ""

if [ "$fails" -gt 0 ]; then
  echo "test-difftest-baseline: $fails failure(s)"
  exit 1
fi
echo "test-difftest-baseline: all passed"
