#### WR-80. `GlobalSurfaceIntersect` fails with `SM_ERR_NULL_POINTER` on two B-spline pairs
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection`. The NULL that triggers the status was not traced.
- **What happens:** The call returns `SM_ERR_NULL_POINTER` (1006) with no curves, although the surfaces are valid and the intersection exists: two NURBS sheets in 4th-order contact along a parabola, and two tube-like arms crossing in an X (two loops with two singular points).
- **Trigger (ssi-corpus cases):** `c2n_quartic_contact_parabola`, `y23_t2_ex5_two_loops`. Verdict `ERROR`.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (failure on valid input).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).
