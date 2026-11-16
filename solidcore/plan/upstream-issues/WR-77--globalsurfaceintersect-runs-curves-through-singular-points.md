#### WR-77. `GlobalSurfaceIntersect` runs curves through singular points instead of splitting them there
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` and its dispatch: the `SmCone` override (cylinder/cylinder), the `SmSphere` override (sphere/cylinder), `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (B-spline and torus pairs). Cause not traced.
- **What happens:** Where intersection branches cross (a singular point of the intersection, with the surfaces tangent there), the output curves pass through the crossing instead of ending at it, so no curve end lies at the singular point and the branch structure is wrong.
- **Trigger (ssi-corpus cases):** `c4_steinmetz` (two equal cylinders; 1 of 2 crossings not split), `c4_viviani_figure8` and `c4n_viviani_nurbs` (Viviani's curve; the double point not split), `y23_fig1a_two_loops_cross`, `y23_t6_ex3_two_crossing_curves`, `y23_t3_ex4_torus_ellipsoid`: verdict `WRONG TOPOLOGY`. In `y23_t2_ex3_loop_two_curves` (8% of a branch covered) and `y23_fig1d_two_quads` (one branch 1% covered) the crossings are also not split, next to missed branches: verdict `INCOMPLETE`. 8 cases.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (wrong topology).
- **Rust port:** not reached yet (`surface.cone_p3`, `surface.sphere`, `intersect.surface_intersector_p5`).
