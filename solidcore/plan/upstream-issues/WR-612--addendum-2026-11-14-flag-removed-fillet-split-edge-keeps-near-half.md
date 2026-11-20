Updates: WR-612

#### WR-612 (update, 2026-11-14). `fillet_split_edge_keeps_near_half` is removed

`FindFilletBrepEdge` keeps the second half of a fillet edge split in two when that half lies on the curve only to the edges' tolerance and its ends lie on the curve too (the fork's ceb3272). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_split_edge_keeps_near_half`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `blend::fillet_geom::p1::tests::wr_612_split_fillet_edge_keeps_its_near_half`.
