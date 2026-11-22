#### UB-9. `N_BasisDerivs` overflows an `int`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisBasic.cpp:3154`, `N_BasisDerivs` (`r *= (p - k);`).
- **What happens:** The factor `p!/(p-k)!` is an `int` and is computed one step past the last row it scales, so it overflows (signed overflow, UB).
- **Trigger:** `p >= 13` with `der >= 10`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
