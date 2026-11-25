#### WR-51. `IsPointOnStartPoint` compares squared distances with a length tolerance
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:7515` `SmSurfaceIntersector::IsPointOnStartPoint`.
- **What happens:** `DistanceBetweenSquared(...) < m_dThisApproxTol3d`: the effective tolerance is `sqrt(tol)`, far looser than intended for small tolerances (and tighter above 1).
- **Trigger:** Any call; e.g. `tol = 1e-6` accepts points 1e-3 apart.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.surface_intersector_p1` difftest, `edge_start_points` and generated `on_start` ops agree with the squared test).
- **Severity:** wrong result.
- **Rust port:** Faithful.
