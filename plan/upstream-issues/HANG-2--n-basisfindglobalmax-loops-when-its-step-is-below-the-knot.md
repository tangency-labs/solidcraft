#### HANG-2. `N_BasisFindGlobalMax` loops when its step is below the knot resolution
- **Severity:** Hang
- **Area:**
- **Location:** `NL_BasisBasic.cpp:1751`, `N_BasisFindGlobalMax`.
- **What happens:** The bracketing step `(U[i+p+1]-U[i])/(10p)` is below half an ulp of the knots, so the loop state never changes.
- **Trigger:** Knots around `1e17` with a span of `1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `InvalidInput` when the state repeats.
