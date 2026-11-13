#### UB-83. `SmSurface::GetDomainWithContinuity` dereferences a missing surface cache
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurface.cpp:12056`, `SmSurface::GetDomainWithContinuity` (base body).
- **What happens:** When `smsurf_GetSurfaceCache` returns `NULL` (neither a trimmed surface cache nor a surface cache can be built), `SE(SM_ERR)` only reports it and the body goes on to call `GetUContinuitiesArray()` through the null pointer.
- **Trigger:** `GetDomainWithContinuity` on a curve-bounded surface or plane whose cache build fails (generated `surface.surface_p1` `dom_cont` cases).
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes (segfault; the `surface.surface_p1` shim now checks the cache first and reports `SM_ERR`).
- **Severity:** crash/UB.
- **Rust port:** Returns the cache build error.
