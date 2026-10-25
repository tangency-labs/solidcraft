#### UB-71. Volume NULL dereferences
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmVolume.cpp:634` `SmVolume::RefreshInvOrientMap` (calls `MakeMapSimple` through NULL without an orient map); `SmTwistVolume.cpp:1106` `SmTwistVolume::EvaluateBoundingBoxSimple` (writes `*pProjBox` unconditionally in the unbounded branch; `(long)(theta / 2 PI)` of NaN); `SmBSplineVolume.cpp:2645` `SmBSplineVolume::InvEvaluateGuessPointSimple` (indexes knots by control-point index; only `SM_ASSERT`).
- **What happens:** NULL dereference, UB conversion, out-of-bounds knot reads.
- **Trigger:** `RefreshInvOrientMap` on a volume with no orient map.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
