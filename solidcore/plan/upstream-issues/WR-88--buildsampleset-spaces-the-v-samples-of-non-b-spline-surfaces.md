#### WR-88. `BuildSampleSet` spaces the `v` samples of non-B-spline surfaces by array index
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SMLib/src/SmGap.cpp:1903`-`1907`, `SmGapFunction::BuildSampleSet`.
- **What happens:** The interior `v` samples are set to `jj / (m_lSampleCntV - 1)` where `jj` is the array index, which starts at `m_lSampleCntU + 1`, not the `v` sample number. The interior `v` parameters are therefore well outside `[0, 1]` (the sample domain is extrapolated).
- **Trigger:** a surface gap function over a non-B-spline surface (for example an `SmCurveBoundedSurface`) with `SetSampleCnt(u, v)`, `v > 2`.
- **How found:** read during porting; compared in fixture `srf_pt_curve_bounded_v_stride` (`fixtures/heal.gap/upstream.json`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`heal::gap::GapFunction::build_sample_set`).
