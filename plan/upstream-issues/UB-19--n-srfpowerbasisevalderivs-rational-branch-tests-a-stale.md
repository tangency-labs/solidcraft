#### UB-19. `N_SrfPowerBasisEvalDerivs` rational branch tests a stale variable
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `NL_SrfPoly.cpp:491`, `N_SrfPowerBasisEvalDerivs`.
- **What happens:** The final loop tests `k1 EQ k2`. `k1` is a leftover loop variable (`(k2+1)*(q+1)`); the intended test is `udr == vdr`. With `udr > vdr` the loop reads past `SD`/`CwDers`.
- **Trigger:** A rational surface with `mfl = NL_TRUE`, `udr = 2`, `vdr = 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Keeps the test so results match. The out-of-range case is `InvalidInput`.
