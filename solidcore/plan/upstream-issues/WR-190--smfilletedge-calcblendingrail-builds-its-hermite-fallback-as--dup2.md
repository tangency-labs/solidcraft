#### WR-190. `SmFilletEdge::CalcBlendingRail` builds its Hermite fallback as a degree-2 curve and drops a control point
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6537-6565` (`SmFilletEdge::CalcBlendingRail`, the `dT1*dT2 < 0.0` branch: `SmBSplineCurve::CreateCanonical(crContext, lCurveDim, 2, sCntrlPoly, ..., sKnotMult = {4, 4}, ...)`).
- **What happens:** The four Bezier points of the cubic Hermite (`GetBezierPoints`) are passed with knot multiplicities 4 at both ends but degree 2. `CreateCanonical` accepts it and makes a quadratic of three points: start, the third Bezier point and end. The second point (start + start tangent / 3) is dropped, so the rail leaves its start in the wrong direction and is not tangent to the first rail. The same branch in `sm_InterpolateEndptsAndTangents` passes degree 3.
- **Trigger:** A blending rail whose end tangents' closest-approach parameters have opposite signs (`smgu_LineLineClosestPoint`: `dT1 * dT2 < 0`).
- **How found:** read during porting (`blend.fillet_geom_p6`, PR #247).
- **Confirmed:** yes: the difftest reaches the branch (83 of 3000 generated cases) and C++ and Rust agree bit for bit on the three-point quadratic.
- **Severity:** wrong result (tangency lost).
- **Rust port:** faithful (degree 2); `blend::fillet_geom::p6` tests pin the three-point result.
