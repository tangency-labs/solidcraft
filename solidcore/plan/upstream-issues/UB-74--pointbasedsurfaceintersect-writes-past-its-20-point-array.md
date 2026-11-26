#### UB-74. `PointBasedSurfaceIntersect` writes past its 20-point array
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurface.cpp:30588` `SmSurface::PointBasedSurfaceIntersect` (UV point version).
- **What happens:** The points go into `SmTsectPnt saTsectPnts[20]`; the size limit and the equal-length check of the two point arrays are `SM_ASSERT`s only, so 20 or more points write past the stack array, and a shorter second array is read past its end.
- **Trigger:** 20 point pairs, or two UV point arrays of different lengths.
- **How found:** read during porting (`intersect.surface_intersector_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** `Surface::point_based_surface_intersect` returns `InvalidInput` for those inputs.
