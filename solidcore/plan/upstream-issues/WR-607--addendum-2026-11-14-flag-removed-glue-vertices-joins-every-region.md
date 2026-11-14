Updates: WR-607

#### WR-607 (update, 2026-11-14). `glue_vertices_joins_every_region` is removed

`SmBrep::GlueVertices` joins the pair of shells of every region both vertices are in (SolidCraft change, 661f79c), and `SmRingFilletRemoval::DoRemoval` relies on it instead of `JoinShellsAtVertex` and no longer clips the tangent faces' domains to their surfaces'. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::glue_vertices_joins_every_region`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/fork_661f79c_glue_every_region.rs` and `tests/fork_52bfef0_ring_fillet_removal.rs`.
