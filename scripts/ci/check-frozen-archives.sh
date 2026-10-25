#!/usr/bin/env bash
# Fails if a retired shared file exists in HEAD's tree, so parallel PRs never conflict
# (AGENTS.md section 5):
# - plan/findings.md and plan/upstream-issues.md, split into one file per record under
#   plan/findings/ and plan/upstream-issues/ (scripts/split_archives.py);
# - crates/difftest/known_mismatches.toml, split into known_mismatches/<unit id>.toml.
# Deleting them passes; re-adding one (e.g. a stale branch that appended to it) fails.
# Usage: check-frozen-archives.sh [<base sha>]   (the base is no longer needed; accepted so
# the CI call stays the same)
set -euo pipefail
fail=0
for f in plan/findings.md plan/upstream-issues.md; do
  if git cat-file -e "HEAD:$f" 2>/dev/null; then
    dir=${f%.md}
    echo "::error file=$f::$f was split into one file per record under $dir/ and must not come back. Put each new entry in its own file under $dir/ (see $dir/README.md); to update an old record, add an addendum file there."
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
