#### WR-161. `SmOffsetSurface` outputs `Buuv` in place of `Buvv` and `Bvvv`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1482` and `:1491`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** `Duvv` is written as `Buuv + d * Nuvv` and `Dvvv` as `Buuv + d * Nvvv`; the base derivatives belong to `Buvv` and `Bvvv`.
- **Trigger:** `Evaluate(uv, 1, 2, ..)` and `Evaluate(uv, 0, 3, ..)`: against central differences of `Duv` and `Dvv` the results are off by 2.1 and 1.2 (relative).
- **How found:** read during porting; finite-difference check of the bit-identical port.
- **Confirmed:** yes (fixture `third_derivative_outputs`).
- **Severity:** wrong result.
- **Rust port:** Kept.
