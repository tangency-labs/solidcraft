#!/usr/bin/env bash
# Fetches the solids report (reports/solids/solids.json) of main's latest CI run.
#
# The report is the primitives example's --json output (crates/solidcore/examples/
# primitives); it is not committed: the CI test job on every push to main uploads it as
# the artifact `solids-report`. This downloads the newest one, taken only from a push run
# of this repo's own .github/workflows/ci.yml on main (same provenance rule as
# scripts/fetch-reports.sh), into reports/solids/. Exits 1 if none is available; the site
# then shows "no data yet". Needs `gh` with read access to Actions (GH_TOKEN in CI).
# Locally: cargo run -p solidcore --example primitives -- --json reports/solids/solids.json
#
#   scripts/fetch-solids.sh [DEST]          # DEST defaults to reports/solids
set -euo pipefail

root=$(git rev-parse --show-toplevel)
dest=${1:-$root/reports/solids}
repo=${GITHUB_REPOSITORY:-tangency-labs/solidcore}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

command -v gh >/dev/null || { echo "fetch-solids: gh not found" >&2; exit 1; }
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
  art=$(gh api "repos/$repo/actions/runs/$id/artifacts?name=solids-report" \
          -q '.artifacts[] | select(.expired | not) | .id' 2>/dev/null | head -n 1 || true)
  [ -n "$art" ] || continue
  if gh run download "$id" -R "$repo" -n solids-report -D "$tmp/run" >/dev/null 2>&1 \
      && [ -s "$tmp/run/solids.json" ]; then
    mkdir -p "$dest"
    mv "$tmp/run/solids.json" "$dest/solids.json"
    echo "fetch-solids: solids.json -> ${dest#"$root"/} (CI run $id)"
    exit 0
  fi
  rm -rf "$tmp/run"
done
echo "fetch-solids: no solids-report artifact from main's CI" >&2
exit 1
