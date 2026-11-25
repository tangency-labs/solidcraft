#### UB-58. `SmCompositeCurve::GetKnots` reads past its array, and loops forever with an interval
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCompositeCurve.cpp:524` (`lLastIndex = rKnots.GetSize()`), `:569` (`for(ii=rKnots.GetSize()-1; ii>=0; ii--)` on an unsigned `ULONG`), `SmCompositeCurve::GetKnots` (def. `:505`).
- **What happens:** For any composite of two or more segments, the first transition reads `rKnots.GetAt(GetSize())` and writes `SetAt(GetSize(), ...)`. With `pOptIvl`, the trim loop never ends and reads `rKnots[ULONG_MAX]`.
- **Trigger:** `GetKnots` on a two-segment composite; again with an interval.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (also a hang).
- **Rust port:** Returns `InvalidInput`.
