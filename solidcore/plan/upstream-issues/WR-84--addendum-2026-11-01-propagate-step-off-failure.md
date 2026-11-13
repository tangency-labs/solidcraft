Updates: WR-84

#### WR-84 (update, 2026-11-01). Opt-in step-off failure propagation in `SmEdgeuse::IsConvexRadialSector`

- **Status in the port:** `Kernel::fixes.edgeuse_convexity_propagates_step_off_failure`, off by default. The default (`false`) keeps `topology::edgeuse::p2::is_convex_radial_sector` bit-identical to pinned v1.0.1 C++ (`Ok(true)` on any non-`Unported` upstream-status error from `evaluate_binormal_step_off`) so `topology.edgeuse_p2` and its fillet/offset callers remain at parity.
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:3592-3653` (`SmEdgeuse::IsConvexRadialSector`, specifically the two `SER(EvaluateBinormalStepOff(...))` calls at `:3619-3620`) at v1.0.1 (`3fd5b95`); in Rust, `crates/solidcore/src/topology/edgeuse/p2.rs` (`is_convex_radial_sector`) and `crates/solidcore/src/kernel.rs` (`UpstreamFixes::edgeuse_convexity_propagates_step_off_failure`).
- **What changes:** When `Kernel::fixes.edgeuse_convexity_propagates_step_off_failure` is `true`:
  1. Both `evaluate_binormal_step_off` call sites inside `is_convex_radial_sector` (`eu` and its `radial` partner) propagate any `Err(e)` directly (`return Err(e)`) instead of turning an upstream-status failure (`ser_as_true(&e)`) into `Ok(true)`.
  2. Every caller in `blend::fillet_corner`, `blend::fillet_solver` and `offset` already threads `k: &Kernel` into `is_convex_radial_sector` and handles `Result<bool>`, so opt-in failure propagation takes effect across all kernel callers without API changes.
  3. Non-status errors (`SolidError::Unported`, null-dereference `SolidError::InvalidInput`, `SolidError::PortInvariant`, `SolidError::NotFound`) continue to propagate unconditionally under both modes, and valid convex and concave radial sectors behave identically across both modes.
- **Regression evidence:** Unit tests in `crates/solidcore/src/topology/edgeuse/p2.rs`:
  - `wr84_flag_default_off_and_lookup_by_name` checks default `false` and `UpstreamFixes::flag_mut("edgeuse_convexity_propagates_step_off_failure")`.
  - `wr84_valid_convex_and_concave_sectors_agree_across_modes` checks valid convex (solid shell) and concave (infinite void shell) edgeuses across `opt_step_off` (`false`/`true`) and `tol_deg` (`0.0`/`2.0`).
  - `wr84_step_off_leaving_face_domain_propagates_under_opt_in` inflates edge tolerance on a concave box sector so `20 * tolerance` steps outside the face UV domain (`SM_ERR` `1001`), verifying default `Ok(true)` false convexity vs. opt-in `Err` propagation.
  - `wr84_radial_partner_step_off_failure_propagates_under_opt_in` shrinks only the radial partner's face UV domain so the first `evaluate_binormal_step_off` succeeds and the second fails, verifying both `SER` sites.
  - `wr84_degenerate_binormal_without_step_off_propagates_under_opt_in` checks `opt_step_off == false` failures (constant zero-tangent B-spline curve, uninitialised B-spline curve, loop-less edgeuse `SM_ERR_INVALID_INPUT` `1007`) and unconditional `InvalidInput` / `NotFound` propagation.
- **Tracking:** tangency-labs/solidcore#2211.
