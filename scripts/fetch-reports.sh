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
keep_tmp=0

# Remove staging on exit unless a failed rollback retained its recovery copy.
cleanup() {
  if [ "$keep_tmp" -eq 1 ]; then
    return 0
  fi
  rm -rf -- "$tmp"
}
trap cleanup EXIT

# Write a consistently prefixed diagnostic to stderr.
warn() {
  echo "fetch-reports: $*" >&2
}

# restore_backup BACKUP REPLACED_TIMES: put the complete pre-publish set back.
restore_backup() {
  local backup=$1 replaced_times=$2 f
  # The complete old set is in BACKUP, so anything in DEST can be replaced.
  # Copy links as links and leave unrelated subdirectories.
  for f in "$dest"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if [ -d "$f" ] && [ ! -L "$f" ]; then continue; fi
    if ! rm -f -- "$f"; then return 1; fi
  done
  if [ "$replaced_times" -eq 1 ] && { [ -e "$dest/times.tsv" ] || [ -L "$dest/times.tsv" ]; }; then
    if ! rm -f -- "$dest/times.tsv"; then return 1; fi
  fi
  for f in "$backup"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if ! cp -P -- "$f" "$dest/"; then return 1; fi
  done
  if [ "$replaced_times" -eq 1 ] && { [ -e "$backup/times.tsv" ] || [ -L "$backup/times.tsv" ]; }; then
    if ! cp -P -- "$backup/times.tsv" "$dest/"; then return 1; fi
  fi
  return 0
}

# Validate, back up and publish a report set; restore it on checked failures.
install() {
  local src=$1 why=$2 f n=0 replaced_times=0 backup= ok=1

  # Check the whole staged set before DEST is touched: at least one report and
  # no directory named *.json. Symlinks are links, never their targets.
  for f in "$src"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if [ -d "$f" ] && [ ! -L "$f" ]; then
      warn "refusing directory named ${f##*/} in $src"
      return 1
    fi
    n=$((n + 1))
  done
  if [ "$n" -eq 0 ]; then
    warn "no reports in $src"
    return 1
  fi
  if [ -e "$src/times.tsv" ] || [ -L "$src/times.tsv" ]; then
    if [ -d "$src/times.tsv" ] && [ ! -L "$src/times.tsv" ]; then
      warn "refusing directory named times.tsv in $src"
      return 1
    fi
    replaced_times=1
  fi

  if ! mkdir -p -- "$dest"; then
    warn "cannot create $dest"
    return 1
  fi
  # A staged report never replaces an unrelated destination subdirectory.
  for f in "$src"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if [ -d "$dest/${f##*/}" ] && [ ! -L "$dest/${f##*/}" ]; then
      warn "refusing to replace directory $dest/${f##*/}"
      return 1
    fi
  done
  if ! backup=$(mktemp -d "$tmp/backup.XXXXXX"); then
    warn "cannot create a backup directory"
    return 1
  fi

  # Back up the reports this publish replaces. cp -P copies symlinks as links.
  for f in "$dest"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if [ -d "$f" ] && [ ! -L "$f" ]; then continue; fi
    if ! cp -P -- "$f" "$backup/"; then
      warn "cannot back up $f"
      return 1
    fi
  done
  if [ "$replaced_times" -eq 1 ] && { [ -e "$dest/times.tsv" ] || [ -L "$dest/times.tsv" ]; }; then
    if ! cp -P -- "$dest/times.tsv" "$backup/"; then
      warn "cannot back up $dest/times.tsv"
      return 1
    fi
  fi

  # Publish. From here on every failure restores the backup.
  for f in "$dest"/*.json; do
    if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
    if [ -d "$f" ] && [ ! -L "$f" ]; then continue; fi
    if ! rm -f -- "$f"; then ok=0; break; fi
  done
  if [ "$ok" -eq 1 ] && [ "$replaced_times" -eq 1 ] \
      && { [ -e "$dest/times.tsv" ] || [ -L "$dest/times.tsv" ]; }; then
    if ! rm -f -- "$dest/times.tsv"; then ok=0; fi
  fi
  if [ "$ok" -eq 1 ]; then
    for f in "$src"/*.json; do
      if [ ! -e "$f" ] && [ ! -L "$f" ]; then continue; fi
      if ! mv -- "$f" "$dest/"; then ok=0; break; fi
    done
  fi
  if [ "$ok" -eq 1 ] && [ "$replaced_times" -eq 1 ]; then
    if ! mv -- "$src/times.tsv" "$dest/"; then ok=0; fi
  fi

  if [ "$ok" -eq 0 ]; then
    if restore_backup "$backup" "$replaced_times"; then
      warn "publish failed; previous reports restored"
      return 1
    fi
    # Keep the recovery copy and its work directory for a person to inspect.
    keep_tmp=1
    warn "publish failed and rollback failed; previous reports are kept in $backup"
    exit 1
  fi

  echo "fetch-reports: $n reports -> ${dest#"$root"/} ($why)"
  return 0
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
