#### UB-30. Reparametrisation writes at negative indices
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_CrvBasic.cpp:5005` `N_CrvReparamToInterval`; `NL_SrfBasic.cpp:5095` `N_SrfReparamToInterval`.
- **What happens:** They write `U[m-p..]` for `m < p` (and `U[r-p..]` for `r < p`).
- **Trigger:** A curve record with `m < p`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`, leaving the curve unchanged.
