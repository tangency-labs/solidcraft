#### HANG-23. Polar set-up of a circle with a far STEP interval shifts one turn per iteration
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmEllipse.cpp:278-289` (`SmPolarConversion::SetUpPolarConversion`, "move to period of greatest overlap").
- **What happens:** Two `while` loops move the NURB's start/end angles by 360 degrees per iteration until their midpoint is within 180 degrees of the STEP interval's. The run time is proportional to the distance, and once the midpoint exceeds about `2^61` degrees adding 360 no longer changes it, so the loop never ends.
- **Trigger:** An `SmCone` built with start and end angles `9223372036854775807` (any cone/cone or cone/plane intersection through `MakeNurb`, which rebuilds the polar converter of its iso circles).
- **How found:** `surface.cone_p3` fuzzing (issue #207, fuzz02755: the Rust side timed out), then a native-only reproduction through `sc_surface_cone_p3_intersect` on Linux.
- **Confirmed:** yes (the native call produced no result within 60 s).
- **Severity:** hang.
- **Rust port:** Returns `NotConverged` after `curve::ellipse::p2::MAX_PERIOD_SHIFTS` (2^20) shifts or when a shift no longer changes the midpoint; the callers ignore that status as upstream ignores the set-up's, leaving the converter impossible.
- **Harness:** the `surface.cone_p3` FFI accepts cone angles only within upstream's documented `[-360, 360]` (`SmCone.h:205-206`). A first guard at about `3.8e8` degrees (the Rust bound) still let fuzzed intersections run past 20 s natively: every surface evaluation of such a cone goes through `SmEllipse::STEPInversion` and `SmExtent1d::PeriodicWrap`, one turn per iteration (HANG-1), confirmed with gdb backtraces.
- **Fuzz sweep trigger (2026-10-20):** `SmSurfOfExtrusion::AdjustSTEPUVDomain` of an ellipse extrusion to a domain whose u end is `9223372036854775807` degrees (`surface.surf_of_extrusion_p1` `fuzz00222`, `fuzz01418`); a revolution (sphere part / torus generator circle) whose v domain bound is `±2^63` or `-1e300` degrees, through `DropPoint` / `ConvertUVFromSTEPToNURBS` (`surface.surf_of_revolution_p3` `fuzz00476`, `fuzz01129`, `fuzz01853`). Confirmed: yes (C++ timeouts; the loop was not traced, so HANG-1 `PeriodicWrap` may be the one spinning). Not guarded yet (open).
- **Fuzz sweep trigger (2026-10-24, PR #750):** `SmSurfOfExtrusion::AdjustSTEPUVDomain`
  of an ellipse extrusion whose minor radius is below `1/DBL_MAX`
  (`5.562684646268003e-309`, so `rx / ry` overflows) with a centre that is not exactly
  `0` hangs C++ (`surface.surf_of_extrusion_p1` `fuzz01428`: centre `2^32`,
  `ry = 5e-324`). Direct probes (8 s timeouts) confirm: every centre
  `1e-16 .. 1e300` hangs while the origin returns via the degenerate branch;
  `ry >= 5.6e-309` passes at any centre up to `1e300`; denormal major radii and
  denormal circles never hang. Inside the subnormal centre edge the hang is
  chaotic (`c = 6e-308/6.5e-308` pass, `6.1/6.3/6.7e-308` hang; `c = 1e-300`
  hangs, `1e-309` passes, both with `ry = 5e-324`), so the loop was not isolated
  (not traced; the point-inversion `SM_CP_POINT_INVERSION` inside the polar
  converter's `ConvertToNURBSParameter` is the suspected non-converging solver).
  Guarded: the FFI rejects the `rx / ry == inf && centre != origin` rectangle.
