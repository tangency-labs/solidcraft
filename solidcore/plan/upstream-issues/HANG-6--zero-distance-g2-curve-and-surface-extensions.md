#### HANG-6. Zero-distance G2 curve and surface extensions
- **Severity:** Hang
- **Area:**
- **Location:** `NL_CrvAdv.cpp:640` `ST_curxcdr` (`NL_G2` parameter search); `NL_SrfAdv.cpp:661` `N_SrfExtendByDist` / `:696` `ST_surxsdr` (G1/G2 extend the iso-curves with `ST_curxcdr` `NL_G2`).
- **What happens:** The step `du = range * ldst / d1` is zero, or no longer changes the parameter, so the search never ends. A negative distance reads outside the knot range.
- **Trigger:** `N_SrfExtendByDist(..., dist = 0, NL_G1, ...)`. A small non-zero distance does it too: `SmOffsetSurface::CreateExtendedSurface` by `1.7e-4` with `SM_CT_G1_G2` over a surface of revolution several units across did not finish.
- **How found:** read during porting; generated difftest case (`surface.offset_surface`).
- **Confirmed:** yes (timeout, the small-distance case).
- **Severity:** hang.
- **Rust port:** Fails with `NotConverged` when a step changes nothing.
- **Harness:** `surface.offset_surface` runs the C++ in process and refuses every
  `CreateExtendedSurface` step (in the FFI, and again in the shim), since a stalled or
  crawling search (e.g. `1e-20` on a unit surface, where each step moves) cannot be
  stopped from outside NLib.
- **Fuzz sweep trigger (2026-10-20):** `N_SrfExtendByDist` with `dist` `0.0`, `-0.0`, `±1e-300` and `cont` `NL_G1`/`NL_G2` (`nlib.srf_adv_p1` `fuzz00349`, `fuzz01120`, `fuzz01972`, `fuzz01998`; `nlib.srf_adv_p2` 14 cases, e.g. `fuzz00123`, `fuzz00438`), or a net coordinate of `±1e300` / `2^63` (iso-curve length overflows; `nlib.srf_adv_p1` `fuzz01548`, `nlib.srf_adv_p2` `fuzz00006`, `fuzz00184`, `fuzz00236` and 8 more); `SmBSplineSurface::CreateExtendedSurface` on a surface with a `1e300` control point (`nurbs.b_spline_surface_n_lib_p1` `fuzz01411`, `fuzz01639`). Confirmed: yes (C++ timeouts). The `nlib.srf_adv_p1`/`p2` FFIs require a non-zero distance whose estimated step `range * dist / iso-polygon bound` is at least `MIN_G2_STEP_EPS = 1024` ulps of the largest knot (`g2_extension_ok`); the `nurbs.b_spline_surface_n_lib_p1`/`p2` FFIs bound coordinates by `MAX_COORD = 1e6`.
- **Fuzz sweep trigger (2026-10-20):** a vanishing but non-zero `NL_G2` distance also hangs `ST_curxcdr` (`NL_CrvAdv.cpp:640`). Each step does change the parameter, but the search takes about `arc length / extension` steps. Fuzz `nlib.crv_adv_p1` fuzz00548 (`closedCurve.smc`, `N_CrvExtendByDist`, `NL_RELATIVE`, `dist = 1e-300`, start, `NL_G2`) and fuzz01037 (`crvTrim1.smc`, same). Rust timed out before the bound. The FFI already rejects G2 extensions below `MIN_G2_FRACTION` (1e-3), so these are not confirmed. The Rust port now also stops after `MAX_G2_STEPS` = 100 000 steps with `NotConverged` (`crv_adv/p1.rs:92`, checks `:700`, `:758`).
