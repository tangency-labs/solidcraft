#!/usr/bin/env bash
# Builds the commits that mirror SolidCraft's solidcore/ to tangency-labs/solidcore, incrementally.
#
# Usage: scripts/ci/solidcore-mirror.sh <solidcore tip> [<main>]   (main defaults to HEAD)
# Prints `state=` (in-sync or fast-forward) and `tip=` (the commit solidcore's main should become)
# for $GITHUB_OUTPUT, and fails on anything else. It creates objects only; it pushes nothing.
#
# A mirror commit is one SolidCraft commit C that changes solidcore/: its tree is exactly
# C:solidcore, its parent is the previous mirror commit, and it keeps C's author, committer, dates
# and message, plus a `solidcraft: <C>` trailer. The last commit of each run also carries
# `solidcraft-main: <main>`, the main commit the run mirrored up to, so the next run walks only
# <that>..main. Commits reached through a merge's second parent (a kernel branch's own commits)
# are mirrored one by one, like main's own.
#
# The first run (no trailer on solidcore's tip) starts from the vendoring merge: the merge on main
# whose second parent is solidcore's tip (git subtree add), so the vendored history maps to itself.
#
# Never `git subtree split`: it replays main's whole history (about 100 minutes here, and it
# overflows dash's recursion limit on Ubuntu).
set -euo pipefail

prefix=${PREFIX:-solidcore}
remote=$(git rev-parse --verify "${1:?solidcore tip}^{commit}")
main=$(git rev-parse --verify "${2:-HEAD}^{commit}")

trailer() { git log -1 --format="%(trailers:key=$1,valueonly,separator=)" "$2" | tr -d '[:space:]'; }

# Where the last run stopped.
last=$(trailer solidcraft-main "$remote")
if [ -z "$last" ]; then
  last=$(git rev-list --merges --parents "$main" | awk -v r="$remote" '$3 == r && !f { print $1; f = 1 }')
  if [ -z "$last" ]; then
    echo "::error::solidcore's tip $remote has no solidcraft-main trailer and is not the second parent of a vendoring merge on main: it was not made by this mirror. This script never rewrites solidcore's history." >&2
    exit 1
  fi
  echo "First mirror run: seeded from the vendoring merge $last." >&2
fi
if ! git cat-file -e "$last^{commit}" 2>/dev/null || ! git merge-base --is-ancestor "$last" "$main"; then
  echo "::error::solidcore's tip $remote was mirrored from $last, which is not in main's history: main was rewritten, or solidcore was pushed to by something else. Not mirroring." >&2
  exit 1
fi
if [ "$(git rev-parse "$last:$prefix")" != "$(git rev-parse "$remote^{tree}")" ]; then
  echo "::error::solidcore's tip $remote does not have the tree of $last:$prefix: the two have diverged. Not mirroring." >&2
  exit 1
fi

tip=$remote
count=0
mirror() { # <commit> [main]: one mirror commit for <commit> on top of $tip
  local c=$1 m=${2:-} prev=$tip tree msg
  if ! tree=$(git rev-parse -q --verify "$c:$prefix"); then
    echo "::error::$c has no $prefix/: refusing to mirror a deleted kernel." >&2
    exit 1
  fi
  msg=$(git log -1 --format=%B "$c" | git interpret-trailers --trailer "solidcraft: $c" ${m:+--trailer "solidcraft-main: $m"})
  tip=$(
    GIT_AUTHOR_NAME=$(git log -1 --format=%an "$c") GIT_AUTHOR_EMAIL=$(git log -1 --format=%ae "$c") \
    GIT_AUTHOR_DATE=$(git log -1 --format=%ad --date=raw "$c") \
    GIT_COMMITTER_NAME=$(git log -1 --format=%cn "$c") GIT_COMMITTER_EMAIL=$(git log -1 --format=%ce "$c") \
    GIT_COMMITTER_DATE=$(git log -1 --format=%cd --date=raw "$c") \
    git commit-tree "$tree" -p "$tip" <<<"$msg"
  )
  # The safety net: a mirror commit is exactly solidcore/ at its commit, on top of the last one.
  if [ "$(git rev-parse "$tip^{tree}")" != "$tree" ] || [ "$(git rev-parse "$tip^@")" != "$prev" ]; then
    echo "::error::the mirror commit of $c is not $c:$prefix on top of $prev" >&2
    exit 1
  fi
  count=$((count + 1))
}

# Every commit since the last run that changes solidcore/, oldest first. History simplification
# follows a merge into the side it took solidcore/ from, so a merged branch's commits are listed.
mapfile -t commits < <(git rev-list --reverse --topo-order "$last..$main" -- "$prefix")
n=${#commits[@]}
for i in "${!commits[@]}"; do
  if [ "$i" -eq $((n - 1)) ] && [ "$(git rev-parse "${commits[$i]}:$prefix")" = "$(git rev-parse "$main:$prefix")" ]; then
    mirror "${commits[$i]}" "$main"
  else
    mirror "${commits[$i]}"
  fi
done
# A merge that resolved solidcore/ to neither side is not listed: then mirror main's tree itself.
if [ "$(git rev-parse "$tip^{tree}")" != "$(git rev-parse "$main:$prefix")" ]; then
  mirror "$(git rev-list -1 --first-parent "$main" -- "$prefix")" "$main"
  [ "$(git rev-parse "$tip^{tree}")" = "$(git rev-parse "$main:$prefix")" ] || { echo "::error::could not reach $main:$prefix" >&2; exit 1; }
fi

# Fast-forward only, and nothing of SolidCraft's: the new commits sit on solidcore's tip, and none
# of them is a SolidCraft commit.
git merge-base --is-ancestor "$remote" "$tip"
for c in $(git rev-list "$remote..$tip"); do
  if git merge-base --is-ancestor "$c" "$main"; then
    echo "::error::$c is a SolidCraft commit; refusing to mirror it." >&2
    exit 1
  fi
done

if [ "$tip" = "$remote" ]; then echo "state=in-sync"; else echo "state=fast-forward"; fi
echo "tip=$tip"
echo "count=$count"
