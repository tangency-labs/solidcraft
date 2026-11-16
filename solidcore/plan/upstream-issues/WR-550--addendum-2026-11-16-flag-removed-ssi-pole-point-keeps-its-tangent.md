Updates: WR-550

#### WR-550 (update, 2026-11-16). `ssi_pole_point_keeps_its_tangent` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, group-B batch intersect). `SmSurfaceIntersector::StepBackFromSingularity` (`step_back_from_singularity_points`, `intersect/surface_intersector/p1.rs`) always leaves each point its own curve tangent; only the surface derivatives and the singular parameter come from the stepped-back point. Removed: the field `UpstreamFixes::ssi_pole_point_keeps_its_tangent`, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260`'s list, and every test setting of it.

- **Tests:** `tests/wr_550_projection_through_sphere_poles.rs` runs on the default kernel (every projected curve within 4e-5, and within 1e-5, of the sphere).
- **Results:** 4 goldens rows move: `surface.surface_p2` `ssi_adv_ssi_cone_cyl1_f0_f0`, `ssi_adv_ssi_cone_cyl2_f0_f0` and `ssi_adv_ssi_cone_plane1_f0_f0` (curves through the cone's apex, refitted) and `topology.brep_p1` `sphere_parallel_proj_cubic` (a projection ending at the sphere's pole). The pull request's class table gives the certified distance bounds of the old and new curves.
