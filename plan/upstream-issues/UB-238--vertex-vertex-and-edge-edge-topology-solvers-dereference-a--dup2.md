#### UB-238. Vertex/vertex and edge/edge topology solvers dereference a `NULL` object in a release `SM_ASSERT`
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmTopologySolver.cpp:369`-`371` `SmTLSVertexVertex::SolveIt` (def. `:365`), `:1343`-`1345` `SmTLSEdgeEdge::SolveIt` (def. `:1339`); the release `SM_ASSERT(a)` is `if(a) {}` (`source/SMLib/inc/SmMessages.h:714`), so its expression is still evaluated.
- **What happens:** `SM_ASSERT(pObj1->IsKindOf(..))` and `SM_ASSERT(pObj2->IsKindOf(..))` run before any `NULL` check. With either object `NULL` the call dereferences it (SIGSEGV), although the solvers they forward to (`SmTLSPointVertex::SolveIt`, `SmTLSCurveEdge::SolveIt`) check the second object with `NER`.
- **Trigger:** `SolveIt(pV, NULL)` or `SolveIt(NULL, pV)` on an `SmTLSVertexVertex`, or the same with edge curves on an `SmTLSEdgeEdge`, in either swap order.
- **How found:** generated difftest case (`intersect.topology_solver_p1 --fuzz 30000`: `/calls/K/1 = -1` and `/calls/K/0 = -1` mutations, kinds vertex/vertex and edge/edge).
- **Confirmed:** yes. SIGSEGV in `SmTLSVertexVertex::SolveIt` in the reference build (19 fuzz cases); the FFI guard now refuses a `NULL` object for both solvers.
- **Severity:** crash/UB (bad argument from calling code).
- **Rust port:** Returns an error instead of dereferencing (`null_deref` for a `NULL` first object, the forwarded solvers' `NULL` check for the second). The difftest guard keeps these calls away from the C++ side.
