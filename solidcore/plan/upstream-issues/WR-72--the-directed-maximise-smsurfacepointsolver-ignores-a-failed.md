#### WR-72. The directed-maximise `SmSurfacePointSolver` ignores a failed `CalculateBoundingBox`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmSurfacePointSolve.cpp:463` (`m_pSrf->CalculateBoundingBox(m_sDomain, &sBBox, NULL, NULL)`), constructor at `:406`.
- **What happens:** The status is not checked. When the box cannot be built (for example a curve-bounded surface without boundary curves, whose surface cache fails on the base `SmSurface::GetKnots`), the box stays in its inside-out `Init()` state and the test point becomes a corner of it, near `±SM_BIG_DOUBLE`.
- **Trigger:** `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE` on a surface whose cache cannot be built.
- **How found:** generated difftest case (`surface.surface_point_solve_p1`, curve-bounded surfaces).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (the box stays `Extent3d::new()`); `SolidError::Unported` still propagates.
