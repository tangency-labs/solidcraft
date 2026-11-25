Updates: WR-511

#### WR-511 (update, 2026-11-14). `extrusion_origin_scaled_zero` is removed

`SmSurfOfExtrusion::AssertValid` test 8 always point-solves the origin on the generating curve with the coordinate-aware zero `SM_EFF_ZERO * (1 + origin.max_dimension())` (tangency-labs/smlib#31). The unscaled `SM_EFF_ZERO` path is gone, with the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** 10 rows of `surface.surface_assert_valid` (`ext0001`, `ext0024`, `ext0025`, `ext0071`, `ext0111`, `ext0175`, `ext0274`, `ext0318`, `ext0323`, `ext0392`) lose their test 8 failure and nothing else; 6 of them become valid. In each the origin is the generator's first control point, which is the clamped curve's start point, so it lies on the curve and the old report was the solver's residual against an unscaled zero. Origins built off the curve (a whole scale unit away) still fail test 8.
- **Tests:** `crates/solidcore/tests/issue_1508_extrusion_origin_scaled_zero.rs`.
