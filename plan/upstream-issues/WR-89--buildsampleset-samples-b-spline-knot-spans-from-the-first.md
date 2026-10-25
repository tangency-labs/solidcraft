#### WR-89. `BuildSampleSet` samples B-spline knot spans from the first knot, not the sample interval
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SMLib/src/SmGap.cpp:1845`-`1859` (`u`), `:1867`-`1881` (`v`).
- **What happens:** After finding the knot indices `lStartIndxU..lEndIndxU` of the sample interval, the span loop walks `sUniqueKnotsU[0..lIvlCntU]` instead of `sUniqueKnotsU[lStartIndxU..lEndIndxU]`, and the normalised values (relative to the whole knot range) are then mapped onto the sample interval with `m_sSampleIvlU.Evaluate`. A sample interval that is not the whole knot range is sampled at the wrong parameters (the loop also overwrites the last end sample set "to avoid numerical tolerances").
- **Trigger:** `SmCrvSrfGapFunction` (or any gap function from B-spline geometry) over an edge interval smaller than the curve's knot range.
- **How found:** read during porting; compared in fixture `crv_pt_bspline_sub_interval`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it.
