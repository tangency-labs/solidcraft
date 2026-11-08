Updates: WR-599

#### WR-599 (update, 2026-10-31). Only the trim-derived cover crosses a wrapped seam (fork b518bbe)

- **Location:** the fork's `SmSurface::CoverCoincidentSurface` wrapped-seam rule
  (tangency-labs/smlib@16bf1cb) as called from `sm_CoverOtherCoincidentFace`
  (`source/SMLib/src/SmEdge.cpp`); not in the pinned SMLib.
- **What happens:** the first cover attempt runs on the faces' stored domains, which are
  conservative and may be their whole surfaces. With the seam rule those cover across the
  seam too, and the merged face takes them as its domain.
- **Trigger:** prog_test `my_test_coaxial_merge_domain` with broad inputs (three half-turn
  faces 1 high on coaxial cylinders r 50, each stored domain its cylinder's whole natural
  domain): the merged face's domain was 360 x 208 for trims spanning 180 x 3.
- **Fix:** tangency-labs/smlib@b518bbe adds `bAcrossWrappedSeams` (default `TRUE`) to
  `CoverCoincidentSurface` and `CoverOtherCoincidentSurface`; `sm_CoverOtherCoincidentFace`
  passes `bRefineDomains`, so the stored-domain attempt stays within one period and only the
  trim-derived one may cross. (The fork's earlier retake, f5b369a/345bec1, is superseded by
  b518bbe and was never ported.) Ported under the same option,
  `Kernel::fixes.cover_wraps_across_closed_seam` (`surface/surface/p3.rs`, `p5.rs`,
  `topology/edge.rs`). Regression test:
  `crates/solidcore/tests/fork_b518bbe_cover_wraps_seam_on_trims_only.rs` (within the
  prog_test's 200 x 4 in both orders and passes; 360 x 208 before).
