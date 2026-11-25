#!/usr/bin/env bash
# Moves a branch's plan/modules.toml edits into plan/units/<id>.toml and drops its
# committed difftest reports, for branches started before the plan was split per unit.
#
# Run it right after merging main into your branch (the merge stops on a modify/delete
# conflict for plan/modules.toml and reports/difftest/*.json; that is expected):
#
#   git fetch origin && git merge origin/main; scripts/units-migrate-branch.sh && git commit --no-edit
#
# What it does:
#   1. Finds the branch tip before the merge (HEAD during a conflicted merge, HEAD^1 if
#      the merge is already committed) and its merge base with main.
#   2. Diffs plan/modules.toml between that merge base and the branch tip, and applies
#      every changed field of every changed unit to plan/units/<id>.toml (main's value
#      stays for fields the branch did not change; a field both sides changed takes the
#      branch's value and is reported as CONFLICT for you to check).
#   3. Removes plan/modules.toml and untracks reports/difftest/ (your local report files
#      stay on disk; they are gitignored now).
#   4. Stages the result. Other conflicts of the merge are left for you to resolve.
#
#   scripts/units-migrate-branch.sh [MAIN_REF]    # MAIN_REF defaults to origin/main
set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"
main_ref=${1:-origin/main}

if git rev-parse -q --verify MERGE_HEAD >/dev/null; then
  tip=HEAD
  other=MERGE_HEAD
elif git rev-parse -q --verify HEAD^2 >/dev/null; then
  tip=HEAD^1
  other=HEAD^2
else
  tip=HEAD
  other=$main_ref
fi
if ! git cat-file -e "$other:plan/units" 2>/dev/null; then
  echo "units-migrate-branch: $other has no plan/units/; merge main first:" >&2
  echo "  git fetch origin && git merge origin/main" >&2
  exit 2
fi
if [ ! -d plan/units ]; then
  echo "units-migrate-branch: plan/units/ missing from the working tree; merge main first" >&2
  exit 2
fi
base=$(git merge-base "$tip" "$other")

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
status=0
if git cat-file -e "$tip:plan/modules.toml" 2>/dev/null; then
  git show "$tip:plan/modules.toml" > "$tmp/tip.toml"
  git show "$base:plan/modules.toml" > "$tmp/base.toml" 2>/dev/null || : > "$tmp/base.toml"
  python3 -B harness/plan/units.py migrate "$tmp/base.toml" "$tmp/tip.toml" > "$tmp/changed" || status=$?
  if [ "$status" != 0 ] && [ "$status" != 3 ]; then
    echo "units-migrate-branch: migration failed" >&2
    exit "$status"
  fi
  if [ -s "$tmp/changed" ]; then
    echo "units-migrate-branch: moved your plan/modules.toml edits into:"
    sed 's/^/  /' "$tmp/changed"
    while read -r f; do
      if [ -e "$f" ]; then git add -- "$f"; else git rm -q --cached --ignore-unmatch -- "$f"; fi
    done < "$tmp/changed"
  else
    echo "units-migrate-branch: the branch did not change plan/modules.toml"
  fi
fi

# plan/modules.toml is gone on main.
if git ls-files --error-unmatch plan/modules.toml >/dev/null 2>&1 || [ -e plan/modules.toml ]; then
  git rm -q -f --ignore-unmatch -- plan/modules.toml
  rm -f plan/modules.toml
fi
# Reports are CI artifacts now: untrack, keep the local files.
if [ -n "$(git ls-files -- reports/difftest)" ]; then
  git rm -r -q --cached --ignore-unmatch -- reports/difftest
fi

python3 -B harness/plan/units.py check >/dev/null
if [ "$status" = 3 ]; then
  echo "units-migrate-branch: check the CONFLICT fields above in plan/units/, then commit" >&2
  exit 3
fi
left=$(git diff --name-only --diff-filter=U)
if [ -n "$left" ]; then
  echo "units-migrate-branch: plan migrated; other conflicts remain:" >&2
  echo "$left" | sed 's/^/  /' >&2
  exit 1
fi
echo "units-migrate-branch: done; commit the merge (git commit --no-edit)"
