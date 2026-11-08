#### UB-7. `N_BasisEval` uses an uninitialised span for an unknown side flag
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisBasic.cpp:2899`, `N_BasisEval` (def. `:2838`).
- **What happens:** `N_BasisFindSpan` returns `NL_CAL_ERR` for a flag that is neither `NL_LEFT` nor `NL_RIGHT`. The return value is ignored and the unset span `i` is used to index `U`.
- **Trigger:** `N_BasisEval(knt, p, u, /*flg=*/0, N, &spn)` with `u` strictly inside the domain.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Takes a `Side` enum, so the case cannot be expressed.
