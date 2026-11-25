Updates: WR-674

#### WR-674 (update, 2026-11-14). `fillet_round_ends_at_tangent_continuation` is removed

A round whose edge runs on tangentially across a vertex into an edge not picked ends at its section through the vertex: the end is left open and noted, and the subtraction cuts the round there by the plane normal to the edge. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_round_ends_at_tangent_continuation`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/issue_2176_round_of_arc_between_tangent_straights.rs`; the test of the path without it is removed.
