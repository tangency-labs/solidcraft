#### UB-10. `N_BasisDerivsArray` walks spans with no bound
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisAdv.cpp:1122`, `N_BasisDerivsArray`.
- **What happens:** Only `u[0]` and `u[n]` go through `N_BasisFindSpan`. The inner spans are found by walking forward with no upper bound, so they read past `U[m]`. The same `int` factor overflow as UB-9 applies.
- **Trigger:** `NL_LEFT` with an inner parameter equal to `U[m-p]` of a clamped vector; or any inner parameter past the end.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
