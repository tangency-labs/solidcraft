#!/usr/bin/env bash
# Fails if the change adds lines to a frozen archive, or brings back a retired shared list. New findings and upstream issues are
# one file each under plan/findings/ and plan/upstream-issues/ so parallel PRs never
# conflict (AGENTS.md section 5). Deleting or shortening archive text is allowed.
# Usage: check-frozen-archives.sh <base sha>   (compares the base tree with HEAD's tree)
set -euo pipefail
base=${1:-}
[ -n "$base" ] || { echo "check-frozen-archives: no base revision, skipping"; exit 0; }
git cat-file -e "$base^{commit}" 2>/dev/null || git fetch -q --depth=1 origin "$base"
fail=0
for f in plan/findings.md plan/upstream-issues.md; do
  # only enforced once the base has the freeze notice (lets the freezing change itself pass)
  # (no `| grep -q`: with pipefail, grep's early exit SIGPIPEs git show and skips the check)
  head_of_base=$(git show "$base:$f" 2>/dev/null | head -c 2000 || true)
  [[ "$head_of_base" == *"Frozen archive"* ]] || continue
  added=$(git diff --numstat "$base" HEAD -- "$f" | awk '{print $1}')
  if [ -n "$added" ] && [ "$added" != 0 ]; then
    dir=${f%.md}
    echo "::error file=$f::$f is a frozen archive; this change adds $added line(s) to it. Put each new entry in its own file under $dir/ (see $dir/README.md)."
    fail=1
  fi
done
# Retired shared lists that must not come back (one file per entry instead).
for f in crates/difftest/known_mismatches.toml; do
  if git cat-file -e "HEAD:$f" 2>/dev/null; then
    echo "::error file=$f::$f is retired; put known mismatches in crates/difftest/known_mismatches/<unit id>.toml (one file per unit, see crates/difftest/src/known.rs)."
    fail=1
  fi
done
exit $fail
