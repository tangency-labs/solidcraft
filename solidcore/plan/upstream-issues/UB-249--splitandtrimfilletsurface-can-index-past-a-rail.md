#### UB-249. `SplitAndTrimFilletSurface` can index past a rail classification
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:4800`-`4866` (`SmFilletSolver::SplitAndTrimFilletSurface`, the interval walk).
- **What happens:** When both current intervals end within `SM_EFF_ZERO_SQ`, both sides advance, but the second branch sets `bFound1 = TRUE` unconditionally after the first branch's search, so a first rail with no further good interval keeps looping with `lStartSearch1` one past its last interval (or on a non-good interval) and reads `(*pCC1)[lStartSearch1]`.
- **Trigger:** Rail classifications whose last good intervals end at the same parameter while the second rail has more good intervals after it.
- **How found:** read during porting (unit `blend.fillet_solver_p3`).
- **Confirmed:** no.
- **Severity:** out-of-bounds read.
- **Rust port:** Faithful flags; an index past the classification is `InvalidInput`.
