#### WR-36. `SmCurve::CurveMaxDistanceBetween` with one sample divides 0/0
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmCurve.cpp:2507` `SmCurve::CurveMaxDistanceBetween`; `SmCrvOnSurf.cpp:1663` `SmCrvOnSurf::SurfaceCurveMaxDistanceBetween`.
- **What happens:** `ii / (lNumSamples - 1)` is `0.0 / 0.0` for one sample, so both curves are evaluated at NaN. `SurfaceCurveMaxDistanceBetween` also sets `bDoPrecise = FALSE` unconditionally, so its precise refinement is dead code.
- **Trigger:** `lNumSamples = 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Computes the same NaN. The harness rejects one sample.
