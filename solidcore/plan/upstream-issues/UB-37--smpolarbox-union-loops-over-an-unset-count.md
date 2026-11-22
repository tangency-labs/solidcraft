#### UB-37. `SmPolarBox::Union` loops over an unset count
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmPolarBox.cpp:1031` `SmPolarBox::Union`; `:930` `GetBoundaryVectors`.
- **What happens:** `GetBoundaryVectors` returns `SM_ERR_INVALID_INPUT` without setting its count when one box is `SM_PS_UNINITIALIZED` (possible after `ReSet`, which keeps a stale basis). `Union` then loops over the unset count.
- **Trigger:** `ReSet` box A, then `A.Union(B)` with aligned stale bases.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
