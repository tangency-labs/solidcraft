Updates: WR-211 (`WR-211--smellipse-adjuststepinterval-collapses-an-arc-moved-a-turn.md`)

#### WR-211 (update, 2026-11-15). `ellipse_step_interval_checks` is removed

`SmEllipse::AdjustSTEPInterval` (`Ellipse::adjust_step_interval`) always refuses (`InvalidInput`) an inverted STEP interval and STEP ends that convert to an empty or non-finite NURBS interval, before it changes the curve, and `SmSurfOfRevolution::UpdateAnalyticalDomain` (`adjust_step_uv_domain_using`) stores the new domain only after the generator accepts it. Upstream installs the collapsed interval: every knot of the rebuilt NURB is one value, and the next reparameterisation divides by its zero length. The flag is gone. Removed: the field, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it.

- **Results:** status `SM_SUCCESS` -> `InvalidInput` (1007), the curve or surface left as it was:
  - `curve.ellipse_p1` `gen0402`: an `Adjust` to `[0, 180.0002]`; upstream's NURB has all eight knots `1.374457`.
  - `surface.surf_of_extrusion_p1`: 14 `review3_*_infinite_*` and `review6_ell_5e-324_*` update cases, whose upstream output is NaN.
  - `surface.surf_of_revolution_p3`: 47 `update` cases, each with a zero-width generator interval (`dom` fractions equal in the generator's direction) or an upstream output with NaN.
- **Results, value change:** `surface.torus`, 6 `issue1433_*` cases: the recognised torus's generator circle keeps its NURB, knots `0, 0, 0, .25, .25, .5, .5, .75, .75, 1, 1, 1` (the full circle of The NURBS Book §7.5), where upstream rebuilt it over a near-full-turn interval that converts to one NURBS parameter (all twelve knots equal).
- **Tests:** `curve::ellipse::p1` (`invalid_step_interval_is_rejected_before_an_ellipse_edit`, `step_interval_checks_allow_turn_preserving_mapping`), `issue_1945_fork_1207_step4.rs`.
