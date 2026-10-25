#### UB-104. Gap records and gap functions dereference null or index before their arrays
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/inc/SmGap.h:371` (`SmVertexEdgeGap::Set`), `source/SMLib/src/SmGap.cpp:134` (`SmGap::GetGapEndPoints`), `:1890`-`1895` and `:1773` (`SmGapFunction::BuildSampleSet`).
- **What happens:** `SmVertexEdgeGap::Set` (and its constructor) caches the gap whenever the parameter is defined, also with a NULL vertex use; `GetGapEndPoints` then calls `GetCurve()` on the NULL edge (`SM_ASSERT_MSG` is compiled out). The same happens for an edge parameter whose edge has no curve, a face UV point whose face has no surface, and `SmVertexEdgeGap::GetEdge` of a vertex use without edge use. `BuildSampleSet` with a sample count of `0` writes `sSampleNormalizedVals[m_lSampleCntU-1]` (index `ULONG_MAX`), or for B-spline geometry computes `(0-1)/n` counts; a `v` count of `0` on a surface writes past the array and returns a pointer into an empty sample array.
- **Trigger:** `SmVertexEdgeGap(NULL, 0.5)`; `SmSrfPtGapFunction(..., 0)` then `GetMaxGapSample`.
- **How found:** read during porting (`heal.gap`).
- **Confirmed:** no (the harness predicts and avoids the null dereferences, and keeps counts at least 1).
- **Severity:** crash/UB (needs a bad argument from the calling code).
- **Rust port:** `heal::gap` returns `InvalidInput`.
