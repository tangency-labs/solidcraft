Updates: WR-402

#### WR-402 (update, 2026-11-09). `taper_pivot_nearest_corner` is removed

A taper extrude whose two links of a corner meet at both ends (a two-curve loop) pivots at the joint nearest the corner's offset curve (`pivot_nearest_corner`), not at `sm_FindPivotPoint`'s first coincident pair. A line and a half circle drafted outward now close: a manifold solid of volume 6555.9139, where upstream's shell does not close. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::taper_pivot_nearest_corner`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/issue_479_taper_pivot.rs` (`taper_pivots_at_the_nearest_corner`), `tests/issue_1449_taper_corner_rules.rs` and the unit tests in `primitive::primitive_creation::p3::ruled`.
