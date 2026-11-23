Updates: WR-385 (`WR-385--cylinder-rebuild-gets-semi-angle-from-rounded-radii.md`)

#### WR-385 (update, 2026-11-15). `cylinder_rebuild_mean_radius` is removed

`SmCone::RebuildSTEPFromNURBParameters` (`Cone::rebuild_step_from_nurb_parameters`) always gives an `SmCylinder` the mean of end radii that agree within `SM_EFF_ZERO_SQRT * (1 + r)` and a zero semi-angle, and refuses (`SM_ERR`) a NURB whose radii differ further, before it changes the analytic state. The flag is gone. Removed: the field, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it; the tests' upstream halves (a semi-angle from rounded radii) are dropped.

- **Results:** `surface.cone_p2` changes in the 8 `issue713_taper_*` cases (a radius-9 cylinder, height 3, its last row scaled by `1 ± 2.2222e-6`, so the radii differ by `2.0e-5`, twice `SM_EFF_ZERO_SQRT * (1 + 9) = 1e-5`): status `SM_SUCCESS` -> `SM_ERR`. Upstream gave the `SmCylinder` a semi-angle of `atan2(2e-5, 3)`, which `SmCylinder::AssertValid` test #0 (zero semi-angle) refuses.
- **Tests:** `surface::cone::p2` (`rebuild_cylinder_*`, `issue_713_cylinder_rebuild_keeps_zero_semi_angle`), the goldens unit's `issue713_fixtures_keep_cylinders`, `fork_5df5b27_ring_of_arcs.rs`, `issue_1606_lean_cylinder_is_a_cone.rs`.
