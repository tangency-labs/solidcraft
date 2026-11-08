#### UB-12. `N_GetCompatibleKnotVectorToTol` reads `U[-1]` and can overflow its buffer
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisBasic.cpp:2483`, `N_GetCompatibleKnotVectorToTol`.
- **What happens:** The "move to average" test reads `U[-1]` when a vector's first knot is above the current smallest knot. It writes into a buffer of `sum(m)+1` slots per vector, which a vector with fewer than `k-1` knots overflows.
- **Trigger:** Two vectors where the second starts above the first's first knot; or `k` vectors one of which has fewer than `k-1` knots.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Skips the test for such vectors. The short-vector case is `InvalidInput`.
