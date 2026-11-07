# Goldens hashes move across platforms: aarch64 darwin vs the CI's x86_64 ubuntu

Found: 2026-11-07, while re-verifying the #2981 flag-retirement lane locally.

## What was observed

A full goldens sweep (`cargo run --profile goldens -p goldens -- --all --jobs 4 --goldens`)
on the current base (7efbd5090) on an aarch64 macOS machine reports 84 of 283 units
failing; main's own CI run at the same code (the goldens job on
blacksmith-*-ubuntu-2404, x86_64) is green. The failing units' changed rows are almost
only **checksum blocks** (`[sum, abs_sum, weighted_sum]` per 256-entry chunk): e.g.
`topology.brep_constructor`'s `hexagon_prism_separate_stitched`, where only the
topology-int block checksum moves (`383570` -> `383763`) while the ops hashes, crash
marker, state, and every measured value are identical. Counts stay the same (`n`).

Reproduced on the clean base: same unit, same case, same single-changed-row shape
(scratch worktree at 7efbd5090, no lane's changes) — so the divergence is not any one
lane's code.

## What it means for local verification

- Value rows (floats) agree across platforms within the 1e-9 basis; the divergences are
  in integer-ordinate sums (B-rep traversal ordinals) and in hash sums built from them.
- A unit's PASS locally on aarch64 is strong evidence for its values, but the canonical
  green is the CI's x86_64 run. When a lane's local sweep fails in units with
  checksum-only movements and identical value/style states, attribute them to the
  platform baseline (this record), not to the lane — check them on the clean base first.
- The 4 units with compare-grade float moves (#2981's lane) still passed locally with
  within-gate values, and their bless rows carry aarch64 values; the block tolerances
  (REL_TOL of the block's absolute sum plus the unit's floor) absorb platform noise
  at 1e-13 scale, which is what aarch64-vs-x86_64 value diffs look like.

## Precedent living on a related edge

`plan/` also carries attempt-budget divergence on aarch64 Linux/GB10 (#2929, the QP
scale search). This record covers darwin-aarch64 vs x86_64 goldens_checksums.
