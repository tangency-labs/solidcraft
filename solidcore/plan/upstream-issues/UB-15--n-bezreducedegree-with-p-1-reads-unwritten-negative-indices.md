#### UB-15. `N_BezReduceDegree` with `p = 1` reads unwritten / negative indices
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_Bezier.cpp:2540`, `N_BezReduceDegree`.
- **What happens:** With `p = 1` it reads the never-written `alf[0]` and `Qw[-1]`.
- **Trigger:** `N_BezReduceDegree` on a degree-1 segment.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for `p < 2`.
