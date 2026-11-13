#### WR-5. `SmExtent3d::Circumscribe` reads the axis map backwards
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmExtent3d.cpp:129`, `SmExtent3d::Circumscribe(SmPseudoBox)`.
- **What happens:** `IsAxisAligned` fills `alAxisMap[i]` with the axis that basis `i` is parallel to, but `Circumscribe` reads it as the basis parallel to axis `i`. Swaps and sign flips come out right; 3-cycles do not.
- **Trigger:** Basis `(y, z, x)` with intervals `[1,2], [3,5], [-7,-6]`: gives min `(3, -7, 1)` instead of `(-7, 1, 3)`.
- **How found:** adversarial / edge-case fixture `circumscribe_axis_cycle_quirk`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
