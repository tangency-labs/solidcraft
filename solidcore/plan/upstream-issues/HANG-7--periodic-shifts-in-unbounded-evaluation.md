#### HANG-7. Periodic shifts in unbounded evaluation
- **Severity:** Hang
- **Area:**
- **Location:** `NL_CrvAdv.cpp:4257` `N_CrvEvalUnboundedPtsAndDerivs`; `NL_SrfAdv.cpp:5446` `N_SrfEvalPtDerivsUnbounded`.
- **What happens:** `while` loops shift the parameter by `ue - us` until it is in range. They never end once the shift no longer changes the parameter, and take ~1e12 steps for far parameters.
- **Trigger:** A G1-closed curve evaluated at `u = 1e20`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `NotConverged` after a bounded number of shifts.
