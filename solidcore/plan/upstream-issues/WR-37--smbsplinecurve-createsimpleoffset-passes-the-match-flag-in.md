#### WR-37. `SmBSplineCurve::CreateSimpleOffset` passes the match flag in the wrong slot
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmBSplineCurve.cpp:2475`, `SmBSplineCurve::CreateSimpleOffset`.
- **What happens:** It calls `ApproximateCurve(..., bOptMatchParameterization, FALSE)`. The caller's match flag lands in `bOptCreateAnalytics`, so parameterisation matching is always off.
- **Trigger:** `CreateSimpleOffset(..., bOptMatchParameterization = TRUE)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
