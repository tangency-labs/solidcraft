#### WR-76. `GlobalSurfaceIntersect` returns tangent curves twice
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection`. Cause not traced; the duplicates suggest tracing from both ends of a tangent branch without merging.
- **What happens:** A tangent branch comes back as two (or more) curves lying on top of each other, sometimes also about 3.4e-5 off the surfaces (approximation tolerance 1e-6). An open tangent line overlaid by its own copy also reads as a closed curve.
- **Trigger (ssi-corpus cases):** `c2n_cylN_on_planeN` (a NURBS cylinder lying on a NURBS plane: two `TANGENT` curves, 100% overlap), `y23_t6_ex1_tangent_line` (100% overlap, up to 3.44e-5 from the surfaces), `y23_t2_ex4_tangential_curve_saddle` (58% overlap, up to 3.40e-5 off): verdict `WRONG TOPOLOGY`. `y23_t6_ex6_four_boundary_curves`: several `CROSSING` curves are copies of one another and branch 1 is only 88% covered: verdict `INCOMPLETE`.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (duplicated topology).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).
