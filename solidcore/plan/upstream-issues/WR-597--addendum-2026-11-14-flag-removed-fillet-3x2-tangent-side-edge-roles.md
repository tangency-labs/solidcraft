Updates: WR-597

#### WR-597 (update, 2026-11-14). `fillet_3x2_tangent_side_edge_roles` is removed

At a 3x2 mixed-convexity corner (with `fillet_3x2_mixed_concave_side_edge`), an unfilleted edge whose faces meet tangentially at the vertex takes the fillets' roles from the convex fillet's face corner (`sm_CheckFaceComplimentaryCornerConcavity`), as tangency-labs/smlib@c17b745, not from `IsConvexRadialSector`'s sign of rounding residues. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_3x2_tangent_side_edge_roles`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `blend::fillet_corner::topology::mixed_3x2::tests::wr_597_tangent_side_edge_takes_roles_from_the_face_corner`.
