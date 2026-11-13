#### UB-248. `sm_RefineRailCurveClassification` works on interval `ii`, not on small interval `ii`, and can read past the classification
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:5596`-`5690` (`sm_RefineRailCurveClassification`).
- **What happens:** The loop counts `ii` from `crSmallIntervals.GetSize()` down to 1 but never reads `crSmallIntervals`: it refines classification interval `ii` itself (so the small intervals TestRollOver found are ignored unless they happen to be intervals `1..n`). It compares `ii` with the interval count taken before any `RemoveAt`, and a refinable interval `ii = size - 1` reads `rRailCurveClassification[ii + 1]`, past the end; after a removal later indices can also be past the end.
- **Trigger:** `TestRollOver` passing more small intervals than (interval count - 2), or a refinable second-to-last interval.
- **How found:** read during porting (unit `blend.fillet_solver_p3`).
- **Confirmed:** no (the refinement path defers at `RailEdgeuseIntersect`, unit `blend.fillet_solver_p2`).
- **Severity:** wrong result / out-of-bounds read.
- **Rust port:** Faithful loop and indexing; an index past the classification is `InvalidInput` instead of a read past the array.
