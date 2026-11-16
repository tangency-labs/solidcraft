#### UB-18. `N_SrfPowerBasisEvalDerivs` overflows `up[NL_MAXDEG+1]`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `NL_SrfPoly.cpp:286` (array), `:315` (`vp = &up[p + 1]`), `:333` (write), in `N_SrfPowerBasisEvalDerivs` (def. `:276`). Also reached from the power-basis `SmBSplineSurface` `EvaluateSimple`.
- **What happens:** The v-power rows are stored after the `p+1` u-powers in the same 18-entry stack array. The writes overrun it when `p + 1 + min(vdr,q)*(q+1) + q > 17`.
- **Trigger:** A bicubic surface with `vdr = 3`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Returns `InvalidInput`.
