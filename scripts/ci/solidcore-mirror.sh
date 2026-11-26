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
# <that>..main. Only main's first-parent commits are mirrored: one per pull request, whether the
# queue squashed it or landed it as a merge commit. A merged branch's own commits (work in
# progress, built on an older main) are not mirrored one by one; its merge commit carries them.
# A merge commit's subject is GitHub's "Merge pull request #N from owner/branch"; its mirror takes
# the pull request's title instead (the first non-empty line of the body) with " (#N)", as a
# squash commit's subject reads, so SolidCore's log reads the same either way.
#
# `scripts/ci/solidcore-mirror.sh --message <commit>` prints the message a commit would be mirrored
# with (before the trailers), and nothing else.
#
# The first run (no trailer on solidcore's tip) starts from the vendoring merge: the merge on main
# whose second parent is solidcore's tip (git subtree add), so the vendored history maps to itself.
#
# Never `git subtree split`: it replays main's whole history (about 100 minutes here, and it
# overflows dash's recursion limit on Ubuntu).
set -euo pipefail

# The message commit $1 is mirrored with, before the trailers. A squash commit (one parent) keeps
# its message. A merge commit "Merge pull request #N from ..." becomes "<first non-empty body line>
# (#N)", then the rest of the body; with no body, or no PR number in the subject, it keeps its own.
message() {
  local c=$1 msg subject n rest title
  msg=$(git log -1 --format=%B "$c")
  if [ "$(git rev-list --no-walk --parents -n 1 "$c" | wc -w)" -le 2 ]; then
    printf '%s\n' "$msg"
    return
  fi
  subject=$(head -n 1 <<<"$msg")
  n=$(sed -n 's/^Merge pull request #\([0-9][0-9]*\)\( .*\)\{0,1\}$/\1/p' <<<"$subject")
  rest=$(tail -n +2 <<<"$msg" | sed '/[^[:space:]]/,$!d')
  title=$(head -n 1 <<<"$rest")
  if [ -z "$n" ] || [ -z "$title" ]; then
    printf '%s\n' "$msg"
    return
  fi
  case "$title" in
    *" (#$n)") printf '%s\n' "$title" ;;
    *) printf '%s (#%s)\n' "$title" "$n" ;;
  esac
  rest=$(tail -n +2 <<<"$rest")
  if [ -n "${rest//[[:space:]]/}" ]; then
    printf '\n%s\n' "$(sed '/[^[:space:]]/,$!d' <<<"$rest")"
  fi
}
if [ "${1:-}" = --message ]; then
  message "$(git rev-parse --verify "${2:?commit}^{commit}")"
  exit 0
fi

prefix=${PREFIX:-solidcore}
remote=$(git rev-parse --verify "${1:?solidcore tip}^{commit}")
main=$(git rev-parse --verify "${2:-HEAD}^{commit}")

# The last value of trailer $1 on commit $2. --if-exists add below: by default git drops a trailer
# whose value equals its neighbour's (solidcraft: X then solidcraft-main: X, when the last solidcore/
# commit is main's tip).
trailer() { git log -1 --format="%(trailers:key=$1,valueonly,separator=%x0a)" "$2" | sed '/^[[:space:]]*$/d' | tail -n 1 | tr -d '[:space:]'; }

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
  msg=$(message "$c" | git interpret-trailers --if-exists add --trailer "solidcraft: $c" ${m:+--trailer "solidcraft-main: $m"})
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

# Every first-parent commit since the last run that changes solidcore/, oldest first. A merge
# counts when solidcore/ differs from its first parent.
mapfile -t commits < <(git rev-list --reverse --first-parent "$last..$main" -- "$prefix")
n=${#commits[@]}
for i in "${!commits[@]}"; do
  if [ "$i" -eq $((n - 1)) ] && [ "$(git rev-parse "${commits[$i]}:$prefix")" = "$(git rev-parse "$main:$prefix")" ]; then
    mirror "${commits[$i]}" "$main"
  else
    mirror "${commits[$i]}"
  fi
done
# Should the walk miss main's tree (it should not with --first-parent), mirror main's tree itself.
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
