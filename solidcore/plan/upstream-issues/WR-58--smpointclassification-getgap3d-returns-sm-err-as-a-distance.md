#### WR-58. `SmPointClassification::GetGap3d` returns `SM_ERR` as a distance
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmPointClass.cpp:464`, `SmPointClassification::GetGap3d` (def. `:418`).
- **What happens:** When the object point has no single drop point on the given surface (even at 100x the zone tolerance), the function, which returns a `double` gap, returns `SM_ERR` (1001.0), which callers read as a large gap.
- **Trigger:** `GetGap3d(pSurface)` for an object point far from `pSurface`.
- **How found:** read during porting.
- **Confirmed:** no (the surface point solve is not ported yet, so these cases defer).
- **Severity:** wrong result.
- **Rust port:** Returns `SM_ERR` as the gap value too.
