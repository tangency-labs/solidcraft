#### UB-173. The base-class box of an extended `SmOffsetSurface` recurses without bound
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:337`, `SmOffsetSurface::CalculateBoundingBox` (def. `:315`) calling `SmSurface::CalculateBoundingBox`, whose `smsurf_GetSurfaceCache` builds the offset's surface cache.
- **What happens:** A box domain not inside the base domain runs the base-class body, which builds the offset's surface cache. Its nodes cover the offset's natural domain, which is the extended surface's once there is one; each node outside the base domain runs the base-class body again, until the stack overflows. This is the pattern of UB-84.
- **Trigger:** An offset allowing extension: `TrimWithDomain` to a part of the base domain, an `EvaluatePoint` outside it (stores an extension), then `CalculateBoundingBox` over the original domain. Also `IsPlanar` (the `SmSurface` body, a box over `GetNaturalUVDomain()`) on an offset of an offset that allows extension, after the outer's `STEPInversion` made the inner store an extension.
- **How found:** crash in the C++ reference run (a generated `surface.offset_surface` case, 30,000-seed run); the `IsPlanar` path in a 100,000-seed run.
- **Confirmed:** yes (stack overflow).
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** Refuses the polar box of the base-class body for such an offset; the surface cache it would build waits on `surface.surface_p6`. The harness keeps these boxes away from the C++ side.
