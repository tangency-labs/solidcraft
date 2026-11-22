#### WR-7. `SmPseudoBox::DistanceToPoint` returns a status as a distance
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmPseudoBox.cpp:772`, `SmPseudoBox::DistanceToPoint`.
- **What happens:** Returns `SM_ERR` (1001.0) as the distance when the point is outside more than three planes. When `smgu_IntersectTwoPlanes` / `ThreePlanes` fail, it uses the `SM_UNDEF_DOUBLE` line or vertex.
- **Trigger:** An inside-out box (`Init()` state).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
