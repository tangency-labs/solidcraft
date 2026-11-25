#### Updates: WR-254 (2026-10-31). The `MakeAnalyticCornerSurface` status exit becomes opt-in

Updates `WR-254--smfilletcorner-makeanalyticcornersurface-reports-its-ser-ner--dup2.md`,
not `WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md` (the other
record that shares the ID).

- **Change:** `SmFilletCorner::MakeAnalyticCornerSurface` returns its `SER` / `NER`
  statuses as an `SmBoolean`, and `SmFilletCorner::CreateCornerPatch` reads the non-zero
  result as `TRUE` ("an analytic surface was made") although none was added, so it skips
  the Coons patch. With the opt-in `Kernel::fixes.analytic_corner_failure_is_false`
  (`UpstreamFixes`, off by default) those exits read `FALSE` and the caller builds the
  patch. Only the exits that return an upstream status change:
  `Err(SolidError::Upstream { .. })` (`SM_ERR` 1001 and `SM_ERR_NULL_POINTER` 1006).
  The port's `Unported`, `InvalidInput` and `PortInvariant` still propagate.
- **Confirmation is historical:** the record's `Confirmed:` refers to the C++ build at
  `c9c5979`; this addendum does not add a new C++ run. The current source pin is
  `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5` (usd-brep 1.0.1,
  `scripts/upstream-pin.env`). The default fixtures are untouched, so the canonical
  difftest gate keeps comparing the pinned behaviour, and no new measured native counts
  are claimed here.
- **Tests:** `crates/solidcore/src/blend/fillet_corner/p2.rs`:
  `analytic_corner_surface_failure_is_false_with_the_fix` (the curveless three-edge
  corner: default `Ok(true)`, flag on `Ok(false)`, no surface either way, corner state
  unchanged), `analytic_corner_surface_failure_flag_keeps_the_false_controls` (two/five
  edges and the cubic rail-rail interpolation, flag off and on),
  `analytic_corner_surface_failure_flag_keeps_invalid_input` (a non-linear chamfer edge
  without fillet geometry), and
  `analytic_corner_surface_failure_flag_does_not_change_a_made_surface` (a three-line
  chamfer plane compared at the same sample points, flag off and on).
- **Scope:** the existing curveless case only exercises the refusal and the unchanged
  corner state. It does not build a Coons patch, and this addendum claims no successful
  Coons result.
- **No new issue:** this is an addendum to WR-254; no `Fixes #254` and no new issue.
