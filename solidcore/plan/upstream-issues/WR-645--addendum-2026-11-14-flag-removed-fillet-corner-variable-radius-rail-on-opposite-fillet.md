Updates: WR-645

#### WR-645 (update, 2026-11-14). `fillet_corner_variable_radius_rail_on_opposite_fillet` is removed

`SmFilletCorner::MakeVariableRadiusFillet` refuses to turn a four-sided corner into a variable-radius fillet when the new fillet's rail leaves the opposite fillet's surface; the corner is then the patch on its exact edges. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_corner_variable_radius_rail_on_opposite_fillet`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/wr_645_drafted_pocket_corner_rounds.rs` (drafted pockets: every corner exact and the body valid; vertical walls: a valid body).
