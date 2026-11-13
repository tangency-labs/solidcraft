#### UB-53. `SmCurve::FindParameterAtArcLength` reads `sKnots[(ULONG)-1]`
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCurve.cpp:3928`, `SmCurve::FindParameterAtArcLength`.
- **What happens:** Stepping backwards from a start within `dParamTol` below the first knot leaves `ik = -1`, which is then used as an index.
- **Trigger:** `dArcLength < 0` from `t = U[0] - 0.5 * dParamTol`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
