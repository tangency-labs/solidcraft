#### UB-51. `sm_UnitizeTangent` reads `rPVV[2]` of a 2-element array
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCurve.cpp:4549` `sm_UnitizeTangent`, called from `:4620` `sm_FilterSolution` with `SmVector3d sPV1[2]` / `sPV2[2]`.
- **What happens:** When the first derivative is not longer than `SM_EFF_ZERO`, the second derivative is read from `rPVV[2]`, past the stack array.
- **Trigger:** A curve whose first and second derivatives both vanish at the solution.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Reads `Vec3::UNDEF` there.
