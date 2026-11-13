#### WR-4. `SmExtent3d::ApproximateUnbounded` computes max from the replaced min
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmExtent3d.cpp:1760`, `SmExtent3d::ApproximateUnbounded` (`SM_BT_UNBOUNDED_MAX` branch).
- **What happens:** Each axis's max is computed from the already replaced min. The optional expanded extent is left untouched when `SetMinMax` rejects it.
- **Trigger:** An extent with infinite min and finite max on one axis.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is. The rejected second extent is `None`.
