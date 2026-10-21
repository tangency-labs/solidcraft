#!/usr/bin/env bash
# Fetches the difftest reports (reports/difftest/<unit>.json) of main's latest CI run.
#
# Reports are not committed: the CI difftest job on every push to main uploads them as
# the artifact `difftest-reports`. This downloads the newest one, taken only from a push
# run of this repo's own .github/workflows/ci.yml on main, into reports/difftest/
# (replacing its *.json). Needs `gh` with read access to Actions (GH_TOKEN in CI).
# When no artifact is available (none uploaded yet, or expired) it falls back to the
# last committed set in git history (needs a non-shallow clone), and exits 1 if that
# fails too. Running difftest locally regenerates the reports of the units you run.
#
#   scripts/fetch-reports.sh [DEST]          # DEST defaults to reports/difftest
set -euo pipefail

root=$(git rev-parse --show-toplevel)
dest=${1:-$root/reports/difftest}
repo=${GITHUB_REPOSITORY:-tangency-labs/solidcore}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

install() {
  local src=$1 n
  n=$(find "$src" -maxdepth 1 -name '*.json' | wc -l)
  [ "$n" -gt 0 ] || return 1
  mkdir -p "$dest"
  find "$dest" -maxdepth 1 -name '*.json' -delete
  find "$src" -maxdepth 1 -name '*.json' -exec mv -t "$dest" {} +
  echo "fetch-reports: $n reports -> ${dest#"$root"/} ($2)"
}

if command -v gh >/dev/null; then
  # Trust only push runs of this repo's own .github/workflows/ci.yml on main. Never
  # select by branch name alone: a fork PR can name its branch "main" too.
  runs=$(gh api "repos/$repo/actions/workflows/ci.yml/runs?branch=main&event=push&per_page=20" \
           -q ".workflow_runs[]
               | select(.event == \"push\" and .path == \".github/workflows/ci.yml\"
                        and .head_branch == \"main\"
                        and .repository.full_name == \"$repo\"
                        and .head_repository.full_name == \"$repo\")
               | .id" 2>/dev/null | head -n 10 || true)
  for id in $runs; do
    art=$(gh api "repos/$repo/actions/runs/$id/artifacts?name=difftest-reports" \
            -q '.artifacts[] | select(.expired | not) | .id' 2>/dev/null | head -n 1 || true)
    [ -n "$art" ] || continue
    if gh run download "$id" -R "$repo" -n difftest-reports -D "$tmp/run" >/dev/null 2>&1 \
        && install "$tmp/run" "CI run $id"; then
      exit 0
    fi
    rm -rf "$tmp/run"
  done
  echo "fetch-reports: no difftest-reports artifact from main's CI" >&2
fi

# Fallback: the set committed before reports moved to CI artifacts.
last=$(git -C "$root" rev-list -1 HEAD -- reports/difftest 2>/dev/null || true)
if [ -n "$last" ]; then
  for c in "$last" "$last^"; do
    if git -C "$root" cat-file -e "$c:reports/difftest" 2>/dev/null; then
      mkdir -p "$tmp/git"
      git -C "$root" archive "$c" reports/difftest | tar -x -C "$tmp/git"
      install "$tmp/git/reports/difftest" "committed in ${c}" && exit 0
    fi
  done
fi
echo "fetch-reports: no reports found" >&2
exit 1
