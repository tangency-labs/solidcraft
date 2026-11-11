#### UB-40. `smgu_PolygonArea` with no points reads `sPoints[-1]`
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmGeomUtility.cpp:274` (`sLastPoint = sPoints[lNumPoints-1]`), `smgu_PolygonArea` (def. `:265`). Reached from `SmPointSet3d::GetArea` (`source/SMLib/inc/SmPointSet.h:358`).
- **What happens:** `lNumPoints - 1` wraps to `ULONG_MAX`.
- **Trigger:** `smgu_PolygonArea` with 0 points; `GetArea` on an empty point sequence.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` (point set: area `-1`).
