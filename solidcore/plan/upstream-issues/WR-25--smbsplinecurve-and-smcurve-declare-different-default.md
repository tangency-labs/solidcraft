#### WR-25. `SmBSplineCurve` and `SmCurve` declare different default `bNonZeroTangents`
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/inc/SmBSplineCurve.h:481` (`bNonZeroTangents=FALSE`) vs `source/SMLib/inc/SmCurve.h:396` (`TRUE`); used in `SmBSplineCurve.cpp:2997` `SmBSplineCurve::Evaluate`.
- **What happens:** Default arguments bind by static type. The recursion inside `SmBSplineCurve::Evaluate` for a zero first derivative with one derivative asked runs with `FALSE` and returns the zero derivative, so the non-zero-tangent fix only works when 2+ derivatives are asked.
- **Trigger:** A B-spline with coincident first two control points: `Evaluate(t0, 1, ...)` through an `SmCurve&`.
- **How found:** adversarial / edge-case fixture `zero_tangent_start_nd1`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Passes `false` there (matches).
