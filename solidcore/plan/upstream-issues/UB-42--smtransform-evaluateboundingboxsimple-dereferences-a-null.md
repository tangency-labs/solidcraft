#### UB-42. `SmTransform::EvaluateBoundingBoxSimple` dereferences a NULL pseudo box
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmTransform.cpp:1224`, `SmTransform::EvaluateBoundingBoxSimple`.
- **What happens:** When an output box is requested, `AreAligned(*pOptParamPseudoBox, ...)` runs before the `NULL` check.
- **Trigger:** The default call (`pOptParamPseudoBox = NULL`) with an output box requested.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
