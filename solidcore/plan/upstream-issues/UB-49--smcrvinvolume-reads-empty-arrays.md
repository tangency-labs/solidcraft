#### UB-49. `SmCrvInVolume` reads empty arrays
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCrvInVolume.cpp:911` `GenerateMults`, `:992` `GetNumberNaturalKnots`, `:623` `CalculateContinuities`.
- **What happens:** `GenerateMults`' state check returns before sizing its output, so `GetNumberNaturalKnots` reads past an empty array. With no breaks, `CalculateContinuities` reads `SetAt(0)` of an empty array and loops on `size - 1` underflow.
- **Trigger:** `GetKnots` with an interval on a cached curve; `CalculateContinuities` with no breaks.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
