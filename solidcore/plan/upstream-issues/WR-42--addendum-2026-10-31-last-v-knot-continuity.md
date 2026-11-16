Updates: WR-42

#### WR-42 addendum (2026-10-31): surface continuity check for last internal V knot

- An opt-in fix is added under `Kernel::fixes.surface_continuity_checks_last_v_knot`
  (default `false`, preserving pinned upstream SMLib v1.0.1 bit-parity).
- **Upstream Location:** `SmSurfaceCache.cpp:1711`. The V direction loop bound used
  `iv + 1 < rVConts.GetSize() - 1` with an erroneous extra `- 1` compared to U
  (`iu + 1 < rUConts.GetSize()`), causing the loop to terminate one iteration early and
  skip the last internal V knot.
- **Implementation:**
  - `crates/solidcore/src/kernel.rs`: Added `pub surface_continuity_checks_last_v_knot: bool`
    to `UpstreamFixes`.
  - `crates/solidcore/src/surface/surface_cache/p1.rs`: Added `surface_continuity_opt_in`
    and `surface_continuity_with_kernel`. When `check_last_v` is true, the loop bound is
    `iv + 1 < v_conts.len()`, evaluating all internal knots. `surface_continuity` retains
    the pinned default behavior (`iv + 1 < v_conts.len() - 1`).
  - `crates/solidcore/src/intersect/surface_tracer.rs`: Wired `cache.surface_continuity_with_kernel(k, &domain)?`.
  - `crates/solidcore/src/surface/surface/p1.rs`: Wired `cache.surface_continuity_with_kernel(k, uv_domain)?`.
  - `crates/solidcore/src/surface/surface/p7.rs`: Wired `surface_continuity_with_kernel(k, uv_domain)?`.
- **Tests:** `wr42_surface_continuity_last_v_knot_skipped_by_default`,
  `wr42_surface_continuity_with_kernel`, and `wr42_surface_continuity_subdomain_filtering`
  in `crates/solidcore/src/surface/surface_cache/p1.rs` verify that default returns C2
  (skipping knot 3.0), whereas opt-in detects C0 at knot 3.0.
