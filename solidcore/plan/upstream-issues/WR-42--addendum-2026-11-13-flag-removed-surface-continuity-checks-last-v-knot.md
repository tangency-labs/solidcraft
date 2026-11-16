Updates: WR-42

#### WR-42 (update, 2026-11-13). `surface_continuity_checks_last_v_knot` is removed

`SmSurfaceCache::GetSurfaceContinuity` checks every internal V knot. That is now the only path: `SurfaceCache::surface_continuity` does it itself, and `surface_continuity_opt_in` and `surface_continuity_with_kernel` are removed. Before, only the `_with_kernel` callers (the surface tracer, `Surface` in `p1` and `p7`) had the fix; the goldens runner called `surface_continuity` and kept upstream's bound. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `surface.surface_cache_p1` changes in 2 cases, both integer continuity codes, and nothing else moves (283 units against main `4f549e08c`, run on Windows). The only entry the change adds to the minimum is the last internal V knot's continuity. In gen0168 the result goes from 1 (`Discontinuous`) to 0 (`Undefined`): that knot is `Undefined`. In gen0222 it goes from 13 (`CInfinity`, no internal knot counted) to 11 (`C1C2G3`): that knot is the only internal one in the domain. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** `wr42_surface_continuity_checks_the_last_v_knot` and `wr42_surface_continuity_subdomain_filtering` in `surface::surface_cache::p1`. The kernel-switch test, `wr42_surface_continuity_with_kernel`, is deleted.
