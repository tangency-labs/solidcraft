Updates: WR-300

#### WR-300 (update, 2026-11-14). `fillet_nx1_end_rib_global_merge` is removed (recorded as WR-303)

At an N-by-1 corner where one rail missed its side edge because that edge ends within the fillet's width (WR-303, the fork's 967f7ab), the end rib is deleted and the executive's global merge cuts the round off where it meets the body. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_nx1_end_rib_global_merge`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/issue_2176_round_of_arc_between_tangent_straights.rs` and the N-by-1 corner tests.
