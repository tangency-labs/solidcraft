#### WR-75. `GlobalSurfaceIntersect` misses tangential intersection curves or reduces them to points
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (`SmSurfaceIntersector.cpp`). Cause not traced.
- **What happens:** Where two surfaces are tangent along a curve, the curve is not traced. Either nothing comes back, or a few `TOUCHING` points or a short piece of the curve.
- **Trigger (ssi-corpus cases):** `c2n_quartic_circle` (a closed tangent circle; no output). `y23_fig1f_high_order_contact` (two `TOUCHING` points; the branch is 1% covered). `y23_t2_ex1_tangent_line` (points and short pieces; 2% covered). `y23_t2_ex2_tangential_curve` (23% covered). Verdict `INCOMPLETE` for all 4.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).
