#### WR-31. `SmEllipse::ConvertTo2D` uses `dThetaXRad` for both axes
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmEllipse.cpp:1740`, `SmEllipse::ConvertTo2D` (def. `:1674`).
- **What happens:** `sNewY` copies the `sNewX` formula with `dThetaXRad`. The projected ellipse gets equal axes, and `SetCanonical` of two parallel vectors runs with its status ignored. `dThetaYRad` is computed and unused.
- **Trigger:** An ellipse whose projected axes are not perpendicular.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
