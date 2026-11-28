#!/usr/bin/env bash
# Regenerates the generated files on main's tip, commits them if they changed, and pushes the
# commit straight to main. Run by .github/workflows/generated.yml from a checkout of main whose
# origin it may push to.
#
# Every try starts clean from main's current tip: any rebase left in progress is aborted, main is
# fetched, the checkout is reset to it, and the files are regenerated from there. Regeneration
# takes about half an hour, and main usually moves in that time, sometimes touching the same
# catalogs or pending files. Rebasing the old regeneration onto it can then conflict, and a
# conflicted rebase left in place fails every later try with "unmerged files"; regenerating on the
# new tip cannot conflict, and a push that loses a race just starts the next try.
#
# Exits 0 when the push lands or nothing changed, 1 when every try lost the race.
set -euo pipefail

# What the job may change: the generated files and the pending files folded into them.
paths=(THIRD-PARTY-LICENSES.txt docs/features.md assets/locales ATTRIBUTION.md attribution)
tries=${GENERATED_TRIES:-3}
pause=${GENERATED_PAUSE:-10}

regenerate() {
  cargo xtask licences # THIRD-PARTY-LICENSES.txt
  cargo xtask features # docs/features.md
  cargo xtask fold-pending # pending locale strings and attribution rows
}

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

for try in $(seq 1 "$tries"); do
  echo "::group::Try $try of $tries"
  if [ -d "$(git rev-parse --git-path rebase-merge)" ] || [ -d "$(git rev-parse --git-path rebase-apply)" ]; then
    git rebase --abort
  fi
  git fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main
  git reset --hard origin/main
  git clean -fdq -- "${paths[@]}"
  echo "Regenerating on main $(git rev-parse --short HEAD)"
  regenerate
  if [ -z "$(git status --porcelain -- "${paths[@]}")" ]; then
    echo "unchanged"
    echo "::endgroup::"
    exit 0
  fi
  git add -A -- "${paths[@]}"
  git commit -q -m "Regenerate generated files and fold pending files [skip ci]"
  if git push origin HEAD:main; then
    echo "Pushed $(git rev-parse --short HEAD) onto main"
    echo "::endgroup::"
    exit 0
  fi
  echo "::endgroup::"
  echo "Try $try: main moved before the push landed; starting again from its new tip."
  [ "$try" -lt "$tries" ] && sleep "$pause"
done
echo "::error::main kept moving: the generated files were not pushed after $tries tries."
exit 1
