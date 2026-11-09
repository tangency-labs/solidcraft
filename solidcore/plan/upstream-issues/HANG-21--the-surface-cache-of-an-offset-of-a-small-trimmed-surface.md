#### HANG-21. The surface cache of an offset of a small trimmed surface does not finish
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:337` (`SmSurface::CalculateBoundingBox` on the offset), `smsurf_GetSurfaceCache` and the `SmSurfaceCache` tree build.
- **What happens:** A box domain outside the trimmed base domain builds the offset's surface cache. For a base about `1e-3` across trimmed to a small part, the build did not finish within 15 s (other cases take milliseconds). The cause was not analysed.
- **Trigger:** A B-spline about `1e-3` across, offset by `7.8e-4`, `TrimWithDomain` to a small part of its domain, then `CalculateBoundingBox` over the original domain.
- **How found:** generated difftest case (`surface.offset_surface`, 30,000-seed run).
- **Confirmed:** yes (timeout).
- **Severity:** hang.
- **Rust port:** Waits on `surface.surface_p6` (the cache tree); the harness keeps these boxes away from the C++ side.
