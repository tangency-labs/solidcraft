#### WR-79. Small closed intersection loops are missed or collapsed to one point
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect`: the `SmPlane` override (plane/sphere) and `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (NURBS paraboloid/plane). Cause not traced.
- **What happens:** A closed loop of radius 1e-4 to 1e-7 is either not found or comes back as one `TOUCHING` point at its centre. This happens even when the radius (1e-4, 1e-5) is far above the approximation tolerance (1e-6).
- **Trigger (ssi-corpus cases):** `c7_tiny_loop_sphere_1e-4`, `_1e-5`, `_1e-6`, `_1e-7` (a sphere dipping just into a plane, a loop of diameter 2e-4 down to 2e-7: one `TOUCHING` point at the loop's centre, as far from the loop as its radius). `c7_tiny_loop_parab_1e-4`, `_1e-5`, `_1e-6`, `_1e-7` (a NURBS paraboloid cut by the plane `z = 1e-8` and the like, just above its apex: no output). Verdict `INCOMPLETE` for all 8.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (`surface.sphere`, `intersect.surface_intersector_p5`).
