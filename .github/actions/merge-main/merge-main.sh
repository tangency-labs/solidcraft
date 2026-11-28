#!/usr/bin/env bash
# Merge the pull request's head onto the current tip of its base branch (see action.yml).
# Needs BASE_REF (the base branch) and HEAD_SHA (the PR's head commit); runs in the checkout.
# BASE_SHA, when set, is the commit of the base branch to merge onto instead of its tip: a job
# that must test the same merge as an earlier job of its run (see action.yml).
#
# The merge base must be exact. actions/checkout leaves a shallow clone, and in a shallow history
# `git merge-base` can answer with a commit at the shallow boundary instead of the real fork
# point; the merge then replays months of main and reports conflicts in files the PR never
# touched. So fetch the whole commit graph first, without blobs (they come on demand, only for
# the files the merge needs), which keeps the fetch small.
set -euo pipefail

: "${BASE_REF:?BASE_REF is not set}"
: "${HEAD_SHA:?HEAD_SHA is not set}"

fetch() {
  local deepen=()
  if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
    deepen=(--unshallow)
  fi
  git fetch -q --no-tags "${deepen[@]}" --filter=blob:none origin \
    "+refs/heads/$BASE_REF:refs/remotes/origin/$BASE_REF" "$HEAD_SHA" ${BASE_SHA:+"$BASE_SHA"}
}

# Prints the conflicting files and returns non-zero when the merge conflicts.
try_merge() {
  base=${BASE_SHA:-$(git rev-parse "origin/$BASE_REF")}
  git checkout -q --detach "$base"
  if git -c user.name=ci -c user.email=ci@localhost merge -q --no-ff --no-edit "$HEAD_SHA"; then
    return 0
  fi
  conflicts=$(git diff --name-only --diff-filter=U)
  echo "Conflicts merging $HEAD_SHA onto $BASE_REF $base:"
  printf '%s\n' "$conflicts" | sed 's/^/  /'
  git merge --abort || true
  return 1
}

fetch
if ! try_merge; then
  # A pinned base cannot move: its conflict is final (the earlier job merged it, so it is rare).
  if [ -n "${BASE_SHA:-}" ]; then
    echo "::error::This PR conflicts with $BASE_REF $BASE_SHA in $(printf '%s' "$conflicts" | paste -sd, - | sed 's/,/, /g')."
    exit 1
  fi
  # The base may have moved under us (a revert landing); look again once before failing.
  echo "Retrying in 30 s against a fresh $BASE_REF."
  sleep 30
  fetch
  if ! try_merge; then
    echo "::error::This PR conflicts with $BASE_REF in $(printf '%s' "$conflicts" | paste -sd, - | sed 's/,/, /g'); merge $BASE_REF into your branch."
    exit 1
  fi
fi
echo "Testing $(git rev-parse HEAD): PR head $HEAD_SHA merged onto $BASE_REF $base"
