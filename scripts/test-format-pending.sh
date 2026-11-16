#!/usr/bin/env bash
# Checks the design-format change files (crates/doc/src/format/): two branches that each add a
# pending change merge without a conflict, and the release fold moves both into the next number.
# A drawing-format change (crates/doc/src/drawing/format/) folds into the drawing's own next number.
# The order they apply in and the upgrades themselves are tested in crates/doc/src/format.rs.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
tmp=$(mktemp -d)
# Cleanup never changes the result of the check. A background git process (auto gc, maintenance) can
# still be writing into .git when this runs ("rm: cannot remove .git: Directory not empty"), so retry
# briefly, and ignore a failure: the temp directory is the system's to clear, the check already ran.
cleanup() {
  local status=$?
  trap - EXIT
  set +e
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    rm -rf "$tmp" 2>/dev/null && [ ! -e "$tmp" ] && break
    sleep 0.2
  done
  exit "$status"
}
trap cleanup EXIT
repo="$tmp/repo"
mkdir -p "$repo/crates/doc/src/drawing" "$repo/scripts"
cp -r "$root/crates/doc/src/format" "$repo/crates/doc/src/"
cp -r "$root/crates/doc/src/drawing/format" "$repo/crates/doc/src/drawing/"
cp "$root/crates/doc/build.rs" "$repo/crates/doc/"
cp "$root/scripts/fold-format-pending.sh" "$repo/scripts/"
rm -f "$repo"/crates/doc/src/format/pending/*.rs "$repo"/crates/doc/src/drawing/format/pending/*.rs  # start from a release: nothing pending
# No background gc or maintenance in the throwaway repo: nothing may still be writing to .git at cleanup.
git() { command git -C "$repo" -c user.name=test -c user.email=test@example.com -c gc.auto=0 -c maintenance.auto=false -c gc.autoDetach=false -c core.fsmonitor=false "$@"; }
git init -q -b main
git add -A
git commit -qm base

change() {
  git switch -q -c "$1" main
  printf '//! %s\n' "$1" > "$repo/crates/doc/src/${2:-format}/pending/$1.rs"
  git add -A
  git commit -qm "$1"
}
change b-second
change a-first
change c-drawing drawing/format

git switch -q main
if ! tree=$(git merge-tree --write-tree a-first b-second); then
  echo "FAIL: two pending format changes conflict" >&2
  exit 1
fi
git merge -q --no-edit a-first b-second >/dev/null
listed=$(git ls-tree --name-only HEAD crates/doc/src/format/pending/ | grep '\.rs$' | sed 's#.*/##' | tr '\n' ' ')
[ "$listed" = "a-first.rs b-second.rs " ] || { echo "FAIL: merged pending changes are: $listed" >&2; exit 1; }
[ "$tree" = "$(git rev-parse HEAD^{tree})" ] || { echo "FAIL: merge-tree and merge disagree" >&2; exit 1; }
git merge -q --no-edit c-drawing >/dev/null

before=$(ls -d "$repo"/crates/doc/src/format/v*/ 2>/dev/null | wc -l)
drawings=$(find "$repo/crates/doc/src/drawing/format" -mindepth 1 -maxdepth 1 -type d -name 'v*' | wc -l)
(cd "$repo" && bash scripts/fold-format-pending.sh) > "$tmp/fold.log"
after=$(ls -d "$repo"/crates/doc/src/format/v*/ | sort -V | tail -1)
[ "$(ls -d "$repo"/crates/doc/src/format/v*/ | wc -l)" -eq $((before + 1)) ] || { echo "FAIL: the fold made no new version" >&2; exit 1; }
[ "$(ls "$after")" = "$(printf 'a-first.rs\nb-second.rs')" ] || { echo "FAIL: the fold moved $(ls "$after")" >&2; exit 1; }
[ -z "$(ls "$repo/crates/doc/src/format/pending/" | grep '\.rs$' || true)" ] || { echo "FAIL: pending changes left after the fold" >&2; exit 1; }
# A drawing's formats start at 1, so its folded ones are v2/, v3/ and so on: the next is its own, never the design's.
drawing_next="$repo/crates/doc/src/drawing/format/v$((drawings + 2))"
[ "$(ls "$drawing_next" 2>/dev/null)" = c-drawing.rs ] || { echo "FAIL: the drawing change did not fold into ${drawing_next##*/}/" >&2; exit 1; }
second=$(cd "$repo" && bash scripts/fold-format-pending.sh)
grep -q "No pending format changes" <<< "$second" && ! grep -q "now fixed" <<< "$second" || { echo "FAIL: a second fold did something" >&2; exit 1; }
echo "ok: pending format changes merge cleanly and fold into $(basename "$after"), a drawing's into ${drawing_next##*/}"
