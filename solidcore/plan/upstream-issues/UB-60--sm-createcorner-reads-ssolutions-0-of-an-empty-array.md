#### UB-60. `sm_CreateCorner` reads `sSolutions[0]` of an empty array
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCompositeCurve.cpp:2178`, `sm_CreateCorner` (def. `:1785`). Reached from `CreateOffsetsOfManyCurves` and `CreateTrimmedOffsets` with `SM_OC_LINEAR_CHAMFER`.
- **What happens:** When the chamfer line misses an extension line, the empty intersection array is read at index 0.
- **Trigger:** Linear-chamfer offsets of random planar polygons hit it often.
- **How found:** crash in the C++ reference run (segfault in the offsetters).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
