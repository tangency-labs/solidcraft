#### HANG-3. Knot merges loop forever on NaN or non-positive tolerance
- **Severity:** Hang
- **Area:**
- **Location:** `NL_BasisBasic.cpp:2483` `N_GetCompatibleKnotVectorToTol` (`tol <= 0`, or NaN knots); `:3665` `N_MakeKnotsCompatible`; `NL_BasisAdv.cpp:253` `N_BasisInsertKnots`; `NL_BasisAdv.cpp:119` `N_GetCompatibleKnotArrayMult`; `NL_SrfAdv.cpp:13051` `N_MergeKnotVectors` (all with NaN knots).
- **What happens:** No knot compares, so no iteration advances.
- **Trigger:** A knot vector containing NaN; `tol = 0`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `InvalidInput`.
