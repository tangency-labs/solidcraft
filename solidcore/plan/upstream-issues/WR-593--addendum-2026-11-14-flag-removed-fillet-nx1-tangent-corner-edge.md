Updates: WR-593

#### WR-593 (update, 2026-11-14). `fillet_nx1_tangent_corner_edge` is removed

`SmFilletNx1Corner::MakeCornerTopology` takes an edge reached past a tangent side edge as the corner's edge when its own faces meet tangentially at the vertex (tangency-labs/smlib@1ff7353), not by the sign of its rounding residues. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_nx1_tangent_corner_edge`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/wr_593_nx1_tangent_corner_edge.rs` (the peg with a 1e-10 sketch residue and the exact one both round to the closed form).
