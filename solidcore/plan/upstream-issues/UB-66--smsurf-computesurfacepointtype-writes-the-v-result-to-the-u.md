#### UB-66. `smsurf_ComputeSurfacePointType` writes the v result to the u output
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurface.cpp:30027`, `smsurf_ComputeSurfacePointType`.
- **What happens:** `if (pOptSurfSideDVV) *pOptSurfSideDUU = sSurfSideDVV`. The v output is never written, and asking for the v side without the u side dereferences NULL.
- **Trigger:** `pOptSurfSideDUU = NULL`, `pOptSurfSideDVV != NULL`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (also a wrong result).
- **Rust port:** Ported as written. The v-without-u request is rejected.
