Updates: WR-576

#### WR-576 (update, 2026-11-09). `linear_sweep_arc_to_cylinder_zero` is removed

`PrimitiveCreation::create_linear_sweep` tests a profile curve for an arc to the zero an `SmCylinder`'s own check holds its generating curve to (`arc_zero`, `1000 * SM_EFF_ZERO * (1 + size)`), not to the B-rep's tolerance, so a curve only near a circle is swept as itself. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::linear_sweep_arc_to_cylinder_zero`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/issue_1507_linear_sweep_arc_zero.rs` (a curve near a circle is swept as its own extrusion, exact circles and text-outline letters as before) and `tests/issue_1507_1477_arc_flags_compose.rs` (with `linear_sweep_arc_is_cylinder` off and on).
