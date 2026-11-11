#### WR-53. `TestSpanAccuracy` compares a tangent length with an angle tolerance
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:3448` `SmSurfaceIntersector::TestSpanAccuracy` (upstream's own `GWC_NEEDS_WORK` note).
- **What happens:** `rNextTSP.CrvDeriv().Length() < m_dSteppingParallelTolRadians` (`SM_EFF_ZERO`): a length is tested against a radian tolerance, so the check only catches (near) zero tangents.
- **Trigger:** A next point with a tangent shorter than `1e-12`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.surface_intersector_p1` difftest, `edge_span_angle`).
- **Severity:** wrong result (unit mix-up).
- **Rust port:** Faithful.
