#### UB-201. `SmApiCreateSphereNoPole` crashes or hangs for small and large radii
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:870` `SmApiCreateSphereNoPole` -> `source/SMLib/src/SmPrimitiveCreation.cpp:6911` `SmPrimitiveCreation::CreateSphereNoPole` (tolerance `SM_ZONE_TOL_3D`). The API only rejects radius magnitudes not above `SM_EFF_ZERO`.
- **What happens:** Erratic below about `1e-4`: radius `1.0000001e-12`, `1e-9`, `1e-6`, `2e-6`, `5e-6`, `2e-5` segfault, `5e-5` and `1e-4` do not return, `1e-5` and `3e-4` and up succeed. From about `4e8` (`4e8`, `5e8`, `1e9`, `1e12`, `1e18`, either sign) it segfaults; `3e8` succeeds.
- **Trigger:** `SmApiCreateSphereNoPole((0,0,0), 2e-5, ..)`; `SmApiCreateSphereNoPole((0,0,0), 4e8, ..)`.
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** crash/UB, hang.
- **Rust port:** not reached yet (deferred); the difftest guard passes only radii in `[3e-4, 3e8]` on to the C++.
