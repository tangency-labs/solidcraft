#### UB-100. `SmPolyBrep::CalculateBoundingBox` dereferences the null point array when asked for a pseudo box
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmPoly.cpp:2734` (`SmPointSet3d sPointSet(*pOptPolyPoints, FALSE)`), `SmPolyBrep::CalculateBoundingBox`.
- **What happens:** In the branch without `pOptPolyPoints`, a requested pseudo box fills a local `sPolyPoints` and then builds the point set from `*pOptPolyPoints`, which is `NULL` in that branch.
- **Trigger:** `CalculateBoundingBox(box, &pseudoBox)` (no point array).
- **How found:** read during porting (`query.poly`).
- **Confirmed:** no (the FFI wrapper rejects the combination).
- **Severity:** crash/UB (null dereference).
- **Rust port:** `PolyBrep::calculate_bounding_box` returns `InvalidInput`.
