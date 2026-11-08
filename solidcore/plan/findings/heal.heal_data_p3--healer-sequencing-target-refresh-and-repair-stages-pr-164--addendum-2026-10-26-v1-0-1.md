# heal.heal_data_p3: healer sequencing claims superseded by v1.0.1 (2026-10-26)

Updates: plan/findings/heal.heal_data_p3--healer-sequencing-target-refresh-and-repair-stages-pr-164.md

The reference moved from c9c5979 to v1.0.1 (3fd5b95). Five claims of the original
finding describe c9c5979 behaviour that v1.0.1 changed. The port now follows v1.0.1:

- **Requesting `Fix_BackPointers` runs the edge cache** (Faithful sequencing).
  Superseded in `SmHealData::HealTgts`. When step 1 is the requested step and not yet
  done, the done step is set to `SM_HO_FIX_BACKPOINTERS` and no later step runs.
  `Fix_BadLoops` gets the same boundary: requesting it records it as done without
  running `Make_UVTrimCurves`. The repair itself stays disabled. Rust:
  `HealData::heal_targets_with`.
- **`Fix_SplitFaceAtSeams` writes the split-*edge* tried list** (Faithful sequencing).
  Superseded in `SmHealData::Fix_SplitFaceAtSeams`. It now writes
  `m_sRanFaceProps_SplitFaceAtSeam`. It still catches new targets up through
  `SM_HO_FIX_SPLITEDGE_ATSEAM`. Rust: `HealData::fix_split_face_at_seams`.
- **Native operational errors from nested healing are ignored** (Bounded catch-up).
  Superseded in `SmHealData::UpdateTargetLists`. The nested `HealTgts` status is now
  returned with `SER` after the recursion count is decremented. Rust:
  `HealData::update_target_lists` propagates the nested error after it restores the
  saved counts and state. The 32-pass bound is unchanged.
- **The successful seam merge sets the tried state; the wire-deletion list keeps the
  last duplicate** (PR #164 review). Superseded in two functions:
  - `SmHealData::Fix_SplitEdgesAtSeam` no longer adds `SM_TRY_RAN` or sets
    `m_eBeenThroughSplitEdgesAtSeam` per merge. It marks each attempted face, splits
    edges crossing interior poles first (`m_sBadFaceProps_CrossedPole`), and catches up
    through `SM_HO_FIX_MOVESEAM`. A final pass then records one `SM_TRY_NOFIX` or
    `SM_TRY_FIXED` per marked face, depending on whether a pole or seam crossing
    remains.
  - `SmHealData::Fix_DegenFaces` no longer removes wire-edge duplicates. It squeezes
    every degenerate face, deletes the remaining wire edges, then deletes the face's
    former vertices that became shell vertices.

  Rust: `HealData::fix_split_edges_at_seam`, `HealData::fix_degen_faces`.
- **Two single-vertex sphere reconstruction cases** (Missing dependencies).
  Superseded in `SmHealData::Fix_DegenFaces`. The closed-surface rebuild
  (`DeleteFace`, then `CreateFaceInRegionFromSurface` and `GlueVertices`) is gone.
  `SmFace::IsDegenerate` excludes a whole closed surface with only vertex loops, and
  `Cache_FaceProps_Stage3` keeps such a face missing a seam active for seam
  insertion (`SmFace::IsClosedSurfaceWithVertexLoops`). Those two cases no longer
  reach the reconstruction path. Their status at v1.0.1 comes from the rerun.

Unchanged: the 32-pass catch-up bound, state restoration on every exit, ignored
statuses of bare `SE` mutation calls (`Unported` still propagates), the native
last-direction edge marking, and the UB-36 and UB-163 harness guards. The gate counts
in the original finding are for c9c5979; the v1.0.1 counts come from the pin rerun.
