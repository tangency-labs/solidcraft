#### WR-50. `AddStartPoint` "replaces" a bounded start point by assigning a local pointer
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:3901` `SmSurfaceIntersector::AddStartPoint`.
- **What happens:** For a bounded/exact start point pair whose stored point is the bounded one, the "replace pTSP with rStartTSP" branch only sets the loop variable `pTSP = &rStartTSP` and reports a match. The exact candidate is dropped and the bounded point stays in `m_vStartPoints`, the opposite of what the comment (and the function header) describe.
- **Trigger:** A bounded start point already in the list, then an exact one within `2 * m_dThisApproxTol3d` with a matching curve tangent.
- **How found:** read during porting.
- **Confirmed:** no (the branch needs `smsurf_EvaluateNormalSection`, which the Rust side does not have yet, so those difftest cases are deferred).
- **Severity:** wrong result.
- **Rust port:** Faithful (the candidate is not added).
