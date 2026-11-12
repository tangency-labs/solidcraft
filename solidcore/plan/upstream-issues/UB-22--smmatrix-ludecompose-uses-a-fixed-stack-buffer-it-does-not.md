#### UB-22. `SmMatrix::LUDecompose` uses a fixed stack buffer it does not check
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `SmMatrix.cpp:662`, `SmMatrix::LUDecompose`.
- **What happens:** It scales with a stack `double[MAX_MATRIX_SIZE]` (1000). Only `SolveLinearSystem` (`:843`) checks the size.
- **Trigger:** `LUDecompose` called directly on a 1001x1001 matrix.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Uses a `Vec`. The harness limits sizes.
