Updates: WR-49

#### WR-49 (update, 2026-11-15). Volume knots with multiplicity above degree are not C0

- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmBSplineVolume::CalculateContinuitiesSimple`, representational
  branch, at [pinned source lines 501–521](https://github.com/NVIDIA-Omniverse/usd-brep/blob/3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5/source/SMLib/src/SmBSplineVolumeNLib.cpp#L501-L521).
  Line 511 sets `lCont` to zero when multiplicity exceeds degree; the following
  expression maps zero to `SM_CT_C0`. The Rust helper `knot_continuity` in
  `crates/solidcore/src/surface/b_spline_volume_n_lib.rs` reproduced this with
  `degree.saturating_sub(mult)`.
- **What happens:** an interior knot whose multiplicity exceeds the degree is
  reported as `C0`, although its representation permits a jump. The endpoint fix
  recorded in the previous addendum does not fix this: both flag settings call
  the same helper for interior knots.
- **Exact rule:** at a knot of multiplicity `m`, a degree-`p` B-spline basis has
  representational continuity `C^(p-m)`, as stated in property 8 of
  [Shene's B-spline basis properties](https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/bspline-property.html).
  When `m > p`, no nonnegative continuity order is guaranteed; the classification
  is `Discontinuous`. When `m == p`, the classification is `C0`. This grades the
  representation, not accidental extra continuity from particular control points.
- **Trigger:** a degree-zero volume in all three directions, each with knots
  `[0, 0.5, 1]`. With `volume_representational_end_continuities` enabled and
  geometric classification disabled, the interior knot has `p = 0`, `m = 1`.
- **Rust confirmation:** an external reproduction ran twice against unchanged
  SolidCraft main `cdd3b92e5`. It returned
  `[Discontinuous, C0, Discontinuous]`, `min_cont == C0`, and
  `has_discontinuities_simple(..., false) == true`. The correct interior class
  and minimum are `Discontinuous`; the Boolean result remains `true` because
  both `C0` and `Discontinuous` are below `G1`.
  Relinking the same reproduction against the fixed library returns
  `[Discontinuous, Discontinuous, Discontinuous]`, minimum `Discontinuous`, and
  Boolean `true`.
- **Upstream confirmation:** static inspection of the pinned source above shows
  the same saturation and classification. No C++ execution was performed for
  this addendum. WR-49's earlier native confirmation remains historical evidence
  for its endpoint defect, not a new execution of this interior-knot case.
- **Correction:** use checked subtraction in `knot_continuity` and return
  `Discontinuous` whenever `m > p`, for every degree. Equality still gives `C0`;
  the existing mappings for positive differences stay unchanged. The correction
  applies with either setting of `volume_representational_end_continuities`.
  It adds no compatibility flag and does not retire or change any flag setting.
  The legacy branch also classifies its first clamped endpoint as
  `Discontinuous`, so its endpoint-inclusive minimum changes accordingly.
  Its last `Undefined` entry and endpoint-inclusive minimum remain separate
  endpoint-layout defects; retiring that branch belongs to #17350.
- **Regression evidence:** before the fix,
  `interior_knot_multiplicity_above_degree_is_discontinuous` failed with actual
  `C0` versus expected `Discontinuous`, while
  `interior_knot_multiplicity_equal_to_degree_is_c0` passed. After the fix, all
  11 tests in the existing module pass. The new tests cover degrees zero through
  three, all axes, both endpoint-flag settings, and direct and wrapped queries.
  The full kernel library test binary passes 4,768 tests, with four ignored.
  All 499 integration test binaries complete: 1,875 passed and 48 ignored.
  Formatting, workspace/all-target Clippy, and the kernel wasm check also pass.
  The post-fix test runs use the `goldens` profile after rebasing onto `8d193fc2a`.
- **Golden evidence:** comparing complete raw outputs before and after the fix
  covers 13,640 cases in two units. All changed scalars are `C0` (`2`) becoming
  `Discontinuous` (`1`); every other raw field is identical.
  - `surface.b_spline_volume_n_lib`: 601 of 2,420 cases change, comprising
    837 directional minima and 1,324 knot classes. Of those classes, 776 are
    first endpoints in the legacy layout and 548 are interior knots. Each
    classification changes because multiplicity exceeds degree; minima reflect
    those lower classes. For example, `conts_cube_e0_rep` changes its minimum
    (`out[1]`) and first endpoint (`out[6]`) from `2` to `1`.
  - `surface.volume_p2`: only `gen1523` changes, among 11,220 cases. Its W minimum
    (`out[10]`) changes from `2` to `1` through the wrapped volume query.
  Both units pass their golden checks after blessing these results, with no
  added, removed, or broken cases. The existing `surface.volume_p2` known failure
  `gen0106` (SolidCraft #16502, an unbend bounding box) remains unchanged.
- **Tracking:** SolidCraft #17549; [fix PR #17619](https://github.com/tangency-labs/solidcraft/pull/17619).
