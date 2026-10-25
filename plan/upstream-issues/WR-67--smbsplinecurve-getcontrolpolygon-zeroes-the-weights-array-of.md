#### WR-67. `SmBSplineCurve::GetControlPolygon` zeroes the weights array of a non-rational curve it was asked to fill
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmBSplineCurve.cpp` (`GetControlPolygon`); read by `SmPolarConversion::SetUpPolarConversion` (`SmCircle` / `SmEllipse`).
- **What happens:** For a non-rational NURB the function sizes the weights array and then `ReSet()`s it, which zeroes the buffer and sets the size to 0. `SetUpPolarConversion` still indexes the buffer (`sWeights[ii*2-2 ..]`), reads `0.0` for every span, and so classifies every span as having constant weights (an exact conversion) and ends with `m_bPolarConversionPossible = TRUE`.
- **Trigger:** `SmSurfOfRevolution::TrimWithDomain` to a very short sweep, line generator `(0.5,0,1)-(3,0,1)` about Z, `swap`, trimmed to `[0, 0.5, 1, 0.5000001]`: the polar circle keeps the given NURB (knots `[0.5,0.5,0.5,0.5000001 x3]`, weights `NL_NOW`) instead of being rebuilt.
- **How found:** PR #51 review (adversarial difftest of `surface.surf_of_revolution_p2`).
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (a differently parameterised polar circle; the geometry is the same).
- **Rust port:** `surface::...::ellipse::p2::set_up_polar_conversion` uses `0.0` weights for a non-rational NURB, as upstream reads them.
