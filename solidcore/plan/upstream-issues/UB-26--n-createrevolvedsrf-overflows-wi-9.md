#### UB-26. `N_CreateRevolvedSrf` overflows `wi[9]`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_SrfCommon.cpp:1408`, `N_CreateRevolvedSrf` (def. `:1398`).
- **What happens:** The circle weights from `N_CalcCircWeights` go into `NL_REAL wi[9]`. A quartic revolution of 241 degrees or more has 13 weights.
- **Trigger:** `ctp = NL_QUARTIC`, `al = 270` degrees.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Keeps all the weights.
