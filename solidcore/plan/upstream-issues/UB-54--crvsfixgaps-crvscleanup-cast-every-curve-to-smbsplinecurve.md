#### UB-54. `CrvsFixGaps` / `CrvsCleanup` cast every curve to `SmBSplineCurve`
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCurve.cpp:1184` `SmCurve::CrvsFixGaps`, `:1106` `SmCurve::CrvsCleanup`.
- **What happens:** The non-virtual `SetControlPoint` (and `TrimGap`) are called through a bad cast for other curve kinds.
- **Trigger:** `CrvsFixGaps` on a list containing an `SmLine` or `SmEllipse`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Moves end points of B-splines only and returns `InvalidInput` for other kinds.
