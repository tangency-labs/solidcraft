#### WR-55. `SmSurface::GetFlatCorners` destroys its return value while filling the corner points
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface.cpp:12691`, `SmSurface::GetFlatCorners` (def. `:12655`).
- **What happens:** The optional-output block tests `if(lRtn &= SM_FC_UMIN_VMIN)` (and so on) with `&=`, so asking for the points masks the flags down to at most one bit (usually none), and later corners are evaluated only if all earlier masks kept a bit.
- **Trigger:** `GetFlatCorners(&points)` on a surface with two or more flat corners.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`Surface::flat_corners`; `IsFlatCorner` is not ported yet).
