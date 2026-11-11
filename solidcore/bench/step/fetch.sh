#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build the STEP reader bake-off corpus in DEST (default bench/step/corpus, gitignored): every
# STEP file of the sources below, folded by SHA-256, one directory per source. Downloads happen
# here, at run time; nothing downloaded is ever committed (the STEP Tools files are "All Rights
# Reserved", the others are someone else's data too).
#
# Sources:
#   solidcraft   SolidCraft's own STEP tests, when this runs inside SolidCraft's tree:
#                tests/step-conformance, tests/kernel-cases/import/*/input.step,
#                tests/step-regressions
#   conformance  github.com/dan335/step-conformance at $CONFORMANCE_REV (CC0 1.0), corpus/
#   solidcore    the step fixtures in this repository (crates/*/tests/fixtures/*.st*p)
#   cadmpeg      github.com/cadmpeg/cadmpeg at $CADMPEG_REV (fixtures and unit-test data)
#   nist-pmi     NIST PMI models, through scripts/fetch-nist-pmi-corpus.sh (SHA-256 pinned)
#   steptools    STEP Tools samples, through scripts/fetch-steptools-corpus.sh (SHA-256 pinned)
#
# Every file keeps its name, prefixed by a counter so that equal names from different places
# stay apart; `.p21` files are kept as written. `manifest.tsv` lists sha256, section, file and
# where it came from.
#
# Usage: bench/step/fetch.sh [DEST]
set -euo pipefail

CONFORMANCE_REV=96f30bb
CADMPEG_REV=bbf22db3c0e100e6c7f5309e6f33d17839ff13e5

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE="$(cd "$HERE/../.." && pwd)"
DEST="${1:-$HERE/corpus}"
# SolidCraft's tree holds solidcore at solidcore/; outside it there is no SolidCraft section.
APP=""
if [ -d "$CORE/../tests/step-conformance" ]; then APP="$(cd "$CORE/.." && pwd)"; fi

mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
WORK="$DEST/.downloads"
mkdir -p "$WORK"
MANIFEST="$DEST/manifest.tsv"
printf 'sha256\tsection\tfile\tsource\n' >"$MANIFEST"
declare -A SEEN=()
n=0

add() { # section, path, source label
  local section="$1" path="$2" source="$3" sha base
  sha="$(sha256sum "$path" | cut -d' ' -f1)"
  if [ -n "${SEEN[$sha]:-}" ]; then return; fi
  SEEN[$sha]=1
  base="$(basename "$path")"
  mkdir -p "$DEST/$section"
  printf -v name '%03d_%s' "$n" "$base"
  n=$((n + 1))
  cp "$path" "$DEST/$section/$name"
  printf '%s\t%s\t%s\t%s\n' "$sha" "$section" "$section/$name" "$source" >>"$MANIFEST"
}

step_files() { find "$1" -type f \( -iname '*.step' -o -iname '*.stp' -o -iname '*.p21' \) -print0 | sort -z; }

if [ -n "$APP" ]; then
  while IFS= read -r -d '' f; do add solidcraft "$f" "${f#"$APP"/}"; done < <(
    step_files "$APP/tests/step-conformance"; step_files "$APP/tests/kernel-cases/import"; step_files "$APP/tests/step-regressions")
fi

if [ ! -d "$WORK/step-conformance/.git" ]; then
  git clone -q https://github.com/dan335/step-conformance "$WORK/step-conformance"
fi
git -C "$WORK/step-conformance" -c advice.detachedHead=false checkout -q "$CONFORMANCE_REV"
while IFS= read -r -d '' f; do add conformance "$f" "step-conformance@$CONFORMANCE_REV/${f#"$WORK/step-conformance"/}"; done < <(
  step_files "$WORK/step-conformance/corpus")

while IFS= read -r -d '' f; do add solidcore "$f" "solidcore/${f#"$CORE"/}"; done < <(
  find "$CORE/crates" -path '*/tests/fixtures/*' -type f \( -iname '*.step' -o -iname '*.stp' \) -print0 | sort -z)

if [ ! -d "$WORK/cadmpeg/.git" ]; then
  git clone -q --filter=blob:none https://github.com/cadmpeg/cadmpeg "$WORK/cadmpeg"
fi
git -C "$WORK/cadmpeg" -c advice.detachedHead=false checkout -q "$CADMPEG_REV"
while IFS= read -r -d '' f; do add cadmpeg "$f" "cadmpeg@${CADMPEG_REV:0:9}/${f#"$WORK/cadmpeg"/}"; done < <(step_files "$WORK/cadmpeg")

"$CORE/scripts/fetch-nist-pmi-corpus.sh" "$WORK/nist-pmi" >/dev/null
while IFS= read -r -d '' f; do add nist-pmi "$f" "nist-pmi/${f#"$WORK/nist-pmi"/}"; done < <(step_files "$WORK/nist-pmi")

"$CORE/scripts/fetch-steptools-corpus.sh" "$WORK/steptools" >/dev/null
while IFS= read -r -d '' f; do add steptools "$f" "steptools/${f#"$WORK/steptools"/}"; done < <(step_files "$WORK/steptools")

echo "fetch: $n files in $DEST (manifest.tsv)"
