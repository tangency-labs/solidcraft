#### UB-17. Banded and triangular matrix routines index outside rows
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `NL_Math.cpp:5714` `N_RealMatrixMultiply`, `:6300` `N_RealMatrixTranspose` (e.g. `:6395`), `:6463` `N_RealMatrixMultiplyPtArray` (`:6538`).
- **What happens:** With an even bandwidth, `j - i + bw/2` reaches `bw`, one column past the row. An in-place banded transpose writes past its new `(n+1)`-column rows when `bw > n + 1`. Non-square triangular operands read outside their rows.
- **Trigger:** A banded matrix with `bw = 2`; an in-place transpose with `bw > n + 1`; a 2x3 triangular operand.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
