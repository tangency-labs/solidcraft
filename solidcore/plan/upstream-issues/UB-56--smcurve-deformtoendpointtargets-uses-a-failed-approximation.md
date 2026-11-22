#### UB-56. `SmCurve::DeformToEndPointTargets` uses a failed approximation
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCurve.cpp:21563`, `SmCurve::DeformToEndPointTargets`.
- **What happens:** It ignores `ApproximateCurve`'s status, then calls the B-spline deform through `*ppOptNewCurve`, which is NULL on failure.
- **Trigger:** A curve kind whose `ApproximateCurve` fails.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns the error.
