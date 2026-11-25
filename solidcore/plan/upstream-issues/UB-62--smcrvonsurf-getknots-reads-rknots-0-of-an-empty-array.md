#### UB-62. `SmCrvOnSurf::GetKnots` reads `rKnots[0]` of an empty array
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCrvOnSurf.cpp:1220`, `SmCrvOnSurf::GetKnots` (def. `:1145`).
- **What happens:** On an uncached curve with `pOptIvl`, an interval holding no UV knot leaves `rKnots` empty, and `rKnots[0]` is read anyway. It also reads `GetAt(lCnt - 1)` when the classification has no interval.
- **Trigger:** `GetKnots` with an interval strictly between two UV knots.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
