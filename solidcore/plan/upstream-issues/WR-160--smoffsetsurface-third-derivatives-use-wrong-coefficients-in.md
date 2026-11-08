#### WR-160. `SmOffsetSurface` third derivatives use wrong coefficients in `Cuuv` and `Cuvv`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1468` and `:1479`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** The derivatives of the normal's numerator `C = Bu x Bv` take `3.0 * Buuv x Buv` in `Cuuv` and `3.0 * Buvv x Buv` in `Cuvv`. Differentiating `Cuu` and `Cvv` gives coefficients `1` and `-1`, so `Duuv` and `Duvv` of the offset are wrong whenever those cross products are not zero.
- **Trigger:** `Evaluate(uv, 2, 1, ..)` on a twisted surface: `Duuv` differs from a central difference of `Duu` by 11 % (relative), while `Duuu` agrees with one of `Duu` to `5e-10`.
- **How found:** read during porting; finite-difference check of the bit-identical port.
- **Confirmed:** yes (fixture `third_derivative_outputs` compares the C++ values with the port).
- **Severity:** wrong result.
- **Rust port:** Kept.
