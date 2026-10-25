#### WR-74. `GlobalSurfaceIntersect` misses isolated tangential contact points
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` (`SmSurface.cpp`) and what it dispatches to: `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (`SmSurfaceIntersector.cpp`) for B-spline pairs, and the `SmSurfOfRevolution` override (inherited by `SmTorus`) for the torus/sphere pair. Cause not traced.
- **What happens:** Two surfaces that touch at single points (normals parallel there, no curve) come back with no output, or with only some of the points. Called with `SmApproxTol3d(1e-6)`, edges on, natural domains.
- **Trigger (ssi-corpus cases):** `c3_sphere_torus_outer`, `c3n_four_points` (0 of 4 points), `c3n_parab_apex`, `c3n_parab_parab_apex`, `y23_t6_ex4_tangential_point`: no output at all. `y23_fig1g_three_points`: one `TOUCHING` point, about 1.9 away from two of the three true points. `y23_t2_ex6_loop_and_point`: the crossing loop is found, the isolated point (0.41 away) is not. Verdict `INCOMPLETE` for all 7.
- **How found:** ssi-corpus, exact ground truth (jyviko's corpus, commit `b04c8a6`, run with difftest `--ssi-corpus`; same verdicts as the corpus' own `score.py`).
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (the general path needs `intersect.surface_intersector_p5`, the torus and sphere are not surface kinds yet); `--ssi-corpus` shows whether the port reproduces it.
