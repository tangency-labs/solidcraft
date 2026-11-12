Updates: WR-725

#### WR-725 (update, 2026-11-11). `closed_smooth_pole_tangent_limit` is removed

`Surface::is_closed_smooth` and `Surface::is_closed_smooth_flag` (`nurbs/srf_nurb.rs`) now always take the G1 test at a station where the derivative across the seam vanishes (a pole on the seam) by its limit, the mixed derivative. That is the only path now. The flag is gone, and so are `Surface::is_closed_smooth_with_fixes` and `Surface::is_closed_smooth_flag_with_fixes`, its only readers. Upstream's angle of the two vanishing vectors is no longer reachable in the Rust kernel; the C++ defect is unchanged in v1.0.1, as recorded above.

- **Before this change the flag reached no library path.** It was read from a `fixes` argument, not from `Kernel::fixes`, and every library caller (`srf_adv::p1::eval_pt_derivs_unbounded` through `is_closed_smooth_flag`, and the `nlib.srf_nurb` goldens unit through `is_closed_smooth`) passed `UpstreamFixes::default()`. SolidCraft's preset line therefore changed nothing. This retirement changes results for every caller, SolidCraft included.
- **Results:** `nlib.srf_nurb` cases `gen0652` and `gen1542` go from status 17 (`NL_GEO_ERR`) to u-closed: a sphere's seam is G1 at its poles. Any other moved rows are listed in the retiring PR with the goldens run on a main export.
- **Tests:** `closed_smooth_pole_of_sphere_is_smooth` and `closed_smooth_pole_noise_does_not_decide` check the default path; their upstream halves are deleted. `issue_2365_nxn_setback_corners.rs` and `issue_2656_blend_regressions.rs` no longer name the flag.
