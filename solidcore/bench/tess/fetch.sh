#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build the mesher bake-off's case folders in DEST (default bench/tess/corpus, gitignored): one
# kernel-case folder (recipe.json, expected.json, input.step) per example design and per STEP
# file. Every input comes from where it lives, at run time; nothing downloaded is committed:
#
#   examples     SolidCraft's example designs: examples/*.json, acceptance/, clean-up/, regressions/
#                (each is a command script; the recipe is the file itself)
#   step-sc      SolidCraft's tests/step-conformance and tests/step-regressions
#   step-core    the step fixtures in solidcore (rod_doubled_edge, discord-2026-10-30-fillet)
#   step-conf    github.com/dan335/step-conformance at $CONFORMANCE_REV, corpus/ (CC0 1.0)
#   step-nist    nine NIST PMI files, through scripts/fetch-nist-pmi-corpus.sh (SHA-256 pinned)
#   step-mc      eight models from github.com/tangency-labs/models (CC0), from the pull requests
#                that add them, until those merge
#
# The kernel-case folders (SolidCraft's own tests/kernel-cases) are read in place by dump.sh.
#
# Usage: bench/tess/fetch.sh [DEST]      (run inside a SolidCraft tree)
set -euo pipefail

CONFORMANCE_REV=96f30bb
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE="$(cd "$HERE/../.." && pwd)"
APP="$(cd "$CORE/.." && pwd)"
DEST="${1:-$HERE/corpus}"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
WORK="$DEST/.downloads"
mkdir -p "$WORK"
EX="$DEST/cases/mesh-bakeoff-excases"
ST="$DEST/cases/mesh-bakeoff-stepcases"
mkdir -p "$EX" "$ST"

example() { # file
  local d="$EX/ex-$(basename "$1" .json)"
  mkdir -p "$d"; cp "$1" "$d/recipe.json"; echo '{}' >"$d/expected.json"
}
step_case() { # name, file
  local d="$ST/$1"
  mkdir -p "$d"; cp "$2" "$d/input.step"; echo '{}' >"$d/expected.json"
  printf '{"commands":[{"command":"file.insert_step","params":{"path":"$CASE/input.step"}}]}\n' >"$d/recipe.json"
}

for f in "$APP"/examples/*.json "$APP"/examples/{acceptance,clean-up,regressions}/*.json; do example "$f"; done
for f in "$APP"/tests/step-conformance/*.step; do step_case "step-sc-$(basename "$f" .step)" "$f"; done
step_case step-sc-rod_doubled_edge "$APP/tests/step-regressions/rod_doubled_edge.step"
step_case step-core-rod_doubled_edge "$CORE/crates/step-brep/tests/fixtures/rod_doubled_edge.step"
step_case step-core-discord-2026-10-30-fillet "$CORE/crates/step-brep/tests/fixtures/discord-2026-10-30-fillet.step"

if [ ! -d "$WORK/step-conformance" ]; then git clone -q https://github.com/dan335/step-conformance "$WORK/step-conformance"; fi
git -C "$WORK/step-conformance" checkout -q "$CONFORMANCE_REV"
C="$WORK/step-conformance/corpus"
for n in bowl_01 door_handle_guide freeform_panel hinge_jig lego_3_pins lego_4x2_01 rail_guide sled_base; do step_case "step-conf-$n" "$C/$n.step"; done
step_case step-conf-plate_900_holes "$C/generated/plate_900_holes.step"
step_case step-conf-torus_ring "$C/generated/torus_ring.step"

"$CORE/scripts/fetch-nist-pmi-corpus.sh" "$WORK/nist-pmi" >/dev/null
N="$WORK/nist-pmi/NIST-PMI-STEP-Files"
for n in nist_ctc_01_asme1_ap242-e1 nist_ctc_02_asme1_ap242-e2 nist_ctc_03_asme1_ap242-e2 nist_ctc_04_asme1_ap242-e1 \
  nist_ctc_05_asme1_ap242-e1 nist_ftc_06_asme1_ap242-e2 nist_ftc_07_asme1_ap242-e2 nist_ftc_10_asme1_ap242-e2; do
  step_case "step-nist-$n" "$N/$n.stp"
done
step_case step-nist-nist_ftc_09_asme1_rd "$N/AP203 geometry only/nist_ftc_09_asme1_rd.stp"

if [ ! -d "$WORK/models/.git" ]; then git clone -q --depth 1 https://github.com/tangency-labs/models "$WORK/models"; fi
for p in "9 coil-flush-cylinder" "20 filleted-boss-grid" "11 spline-profile-prism" "18 tessellated-brep-mix" \
  "17 toothed-plate-50k-faces" "10 twisted-sweep-tube" "8 untrimmed-surface-body" "15 vertices-off-curves"; do
  set -- $p
  git -C "$WORK/models" fetch -q --depth 1 origin "pull/$1/head"
  git -C "$WORK/models" show "FETCH_HEAD:step/$2/$2.step" >"$WORK/$2.step"
  step_case "step-mc-$2" "$WORK/$2.step"
done
echo "fetch: $(ls "$EX" | wc -l) example designs, $(ls "$ST" | wc -l) STEP files in $DEST/cases"
