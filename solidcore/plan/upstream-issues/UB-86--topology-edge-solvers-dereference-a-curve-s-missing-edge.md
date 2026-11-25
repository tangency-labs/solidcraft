#### UB-86. Topology edge solvers dereference a curve's missing edge
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmTopologySolver.cpp:557`/`:582` `SmTLSPointEdge::SolveIt`, `:1079`/`:1101` `SmTLSCurveEdge::SolveIt`, `:1359`/`:1380` `SmTLSEdgeEdge::SolveIt`.
- **What happens:** `pCurve->GetEdge()` is only checked by an `SM_ASSERT_MSG` (compiled out); with no shape (`m_pShape == NULL`) a curve that defines no edge is dereferenced as `NULL->GetInterval()`.
- **Trigger:** Any of these local solvers called on a free curve (no owning edge) outside a shape solve.
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the difftest guard passes only curves that own an edge).
- **Severity:** crash/UB (bad argument from calling code).
- **Rust port:** Returns `InvalidInput`.
