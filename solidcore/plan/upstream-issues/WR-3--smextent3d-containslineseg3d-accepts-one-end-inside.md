#### WR-3. `SmExtent3d::ContainsLineSeg3d` accepts one end inside
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmExtent3d.cpp:1235` (`bRtn |= ContainsPoint3d(rEndPoint, dTol)`), `SmExtent3d::ContainsLineSeg3d` (def. `:1227`).
- **What happens:** The comment at `:1233` says the box contains the segment if it contains both points, but the code ORs the two tests.
- **Trigger:** A segment from inside to far outside the box returns TRUE.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
