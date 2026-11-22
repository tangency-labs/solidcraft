#### UB-8. Span and basis routines read outside short knot vectors
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisBasic.cpp:1429` `N_BasisFindSpan`, `:2838` `N_BasisEval` (stack arrays `left/right[NL_MAXDEG + 2]` at `:2846`).
- **What happens:** With `m < 2p+1` the routines read outside `U`. With `p > NL_MAXDEG + 1`, `N_BasisEval` overflows its stack arrays.
- **Trigger:** `p = 3` with knots `[0,0,0,1,1]`; or `p = 20`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
