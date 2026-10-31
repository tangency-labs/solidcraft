Updates: WR-446

#### WR-446 addendum (2026-10-31). The port's opt-in fix stops `ValidatePointers` making UV trims
- **Severity:** Wrong result
- **Area:** Topology
- **What changed:** `topology::brep::validation::pointers` now passes
  `ok_to_make_uv_trim_curve = FALSE` to the clockwise neighbour's
  `SmEdgeuse::NormalizedEvaluate` in `sm_CheckEdgeuseGeometry` when the new
  default-off `Kernel::fixes.validation_does_not_make_uv_trims` is set. The self
  evaluation, the neighbour's 3D evaluation and its drop onto the existing face are
  unchanged, and a failing evaluation still propagates its status. This is not the
  WR-406 trim-reversal policy (`uv_trim_reversal_on_surface`): validation still
  repairs existing trims as before, and no derivative or geometry-repair path is
  touched.
- **Default:** pinned C++ parity (flag off): `ValidatePointers` still creates the
  neighbour's UV trim curve.
- **Evidence (port, measured):** the focused test
  `topology::brep::validation::pointers::issue_891_tests::wr446_tests` (three tests)
  runs the public `validate_pointers` on the existing `half_disc` fixture, built
  through the real `MakeFaceWithCurves` from the rational half-circle arc and the
  linear chord on the 60 x 4 plane. It starts with the two stored arc trims
  (`EdgeuseId(0)`, `EdgeuseId(1)`) and the chord's edge use untrimmed; the default
  check stores the chord's trim (the stored set grows from two to three), while with
  the flag the stored set stays at two and no new trim is made. The status stays
  `SM_SUCCESS` and no `AssertArray` reports are
  emitted (level 2, `walk = true`) in either flag state. A 2 x 2 matrix over
  `uv_trim_reversal_on_surface` false/true leaves the existing arc trims equal when
  that flag is held fixed. The all-present positive control (every edge use given a
  trim through the public evaluator) and the wholly-untrimmed control (the check
  returns at its first test) are identical in both flag states. Focused run: 3
  passed, 0 failed, 2698 filtered.
- **C++ record:** the original entry's evidence is the historical gdb session from
  the original investigation (PR #1000), not a fresh native run for this addendum;
  its line numbers are for that pin (v1.0.1, `3fd5b954`) and are left unchanged
  here. This addendum does not claim a new native comparison.
- **Scope limit:** the flag claims only that `ValidatePointers` makes no new UV trim.
  It does not make validation read-only in general, does not change `EndShell`, and
  does not serialise UV-trim presence in the `topology.brep_constructor` fixtures.
- **Upstream:** unchanged; this is the port's opt-in fix, not an upstream fix.
