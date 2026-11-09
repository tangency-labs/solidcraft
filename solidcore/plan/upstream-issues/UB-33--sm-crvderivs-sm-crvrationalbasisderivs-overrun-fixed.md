#### UB-33. `sm_CrvDerivs` / `sm_CrvRationalBasisDerivs` overrun fixed derivative arrays
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `SmNurbsCrv.cpp:1281`-`1288` (`sm_CrvDerivs`: `BD_STACK[GW_MAX_DERIV]`, but the guard is `der > GW_MAX_DERIV`); `SmNurbsCrv.cpp:1182` `sm_CrvRationalBasisDerivs`, reached from `SmOffsetCurve::Evaluate` (`SmOffsetCurve.cpp:141`).
- **What happens:** `der == 5` writes past the row arrays. A zero-distance offset of a rational B-spline evaluated with more than `NL_MAXDER` (4) derivatives overflows a fixed stack array.
- **Trigger:** `sm_CrvDerivs(..., der = 5, ...)`; `SmOffsetCurve` (distance 0, rational base) `Evaluate` with 5 derivatives.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Rejects `der >= 5`. The harness limits offset evaluation to 4 derivatives.
