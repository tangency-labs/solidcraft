#### UB-96. `SmTLSEdgeVertex::SolveIt` treats its vertex operand as a curve
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmTopologySolver.cpp:1014`-`1037`, specifically the `pCurve` assignments at `:1032`-`:1033`.
- **What happens:** After selecting the vertex from pObj2 (normal order) or pObj1 (swapped), the method casts that SAME object to SmCurve and calls GetEdge. A SmVertex is not a SmCurve, so this is an invalid downcast/member access, regardless of the caller's otherwise correct curve/vertex argument order.
- **Trigger:** `SmTLSEdgeVertex::SolveIt` with a real edge curve and vertex, in either argument order.
- **How found:** read during porting `intersect.topology_solver_p2` (PR #60).
- **Confirmed:** no. SolveIt is deliberately not executed. The C++ reference runtime-type probe `sc_intersect_topology_solver_p2_edge_vertex_operand` constructs a real shell vertex and confirms `IsKindOf(SmCurve_TYPE)` is false. The undefined SolveIt cast itself is deliberately NOT executed; no particular crash or numeric outcome is claimed.
- **Severity:** undefined behavior.
- **Rust port:** validates the selected operand's class and returns InvalidInput. Separate native class-probe and Rust regressions cover both orders; there is no fabricated native solver answer.
