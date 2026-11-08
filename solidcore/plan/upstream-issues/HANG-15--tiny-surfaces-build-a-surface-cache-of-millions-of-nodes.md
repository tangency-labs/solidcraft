#### HANG-15. Tiny surfaces build a surface cache of millions of nodes
- **Severity:** Hang
- **Area:**
- **Location:** `SmSurfaceCache.cpp:4495` `SmSurfaceCache::SubdivideToTolerances` (stops only at `m_dMinimumSideLengthRatioUV`, `:3932`-`:3949`), through `smsurf_GetSurfaceCache` from `SmSurface::GlobalLineIntersect` / `GlobalCurveIntersect`.
- **What happens:** The cache tolerances are absolute. A noisy cubic B-spline surface about `5e-4` across fails the angle test down to the minimum side ratio in both directions, so every patch splits to about `2^20` leaves.
- **Trigger:** `GlobalCurveIntersect` of a line with a 5 x 6 net of scale `1e-4` (control points jittered by 20 %, z noise `1e-8`), sliver domain `[1.6112222, -0.986, 1.6112223, 1.07]`: 15 s in the C++, 121 s in the port. The same net scaled by `1e3` takes 4 ms.
- **How found:** generated difftest case (`surface.surface_p8`; the generator seemed to hang).
- **Confirmed:** yes (C++ 15.4 s, Rust 121.5 s for the one case).
- **Severity:** hang (minutes per call; bounded by the minimum side ratio).
- **Rust port:** Same bound (`MAX_SUBDIVISION_DEPTH` does not limit the node count). The generator uses scale `1e-3` instead of `1e-4`.
