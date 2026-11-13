#### WR-92. `SmVertex::IsTopologicalVertex` reports a failed tangent evaluation and cusps as topological
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmVertex.cpp:716` `SmVertex::IsTopologicalVertex` (`SER(pEdgeN->GetEndTangent(TRUE, ...))`, and the angle test `fabs(dAngleRad - SM_PI) > dAngLimitRad`).
- **What happens:** As WR-52, `SER` returns the failing `SmStatus` of `GetEndTangent` at an edge's start as the `SmBoolean` result, so the vertex counts as topological. The angle test also accepts edges meeting at `PI` (a cusp, the curves doubling back), which upstream flags itself ("gwc: is this a bug? it counts cusps").
- **Trigger:** two different curves meeting at equal parameters whose start tangent cannot be evaluated; two edges doubling back on each other at the vertex.
- **How found:** read during porting (unit `topology.vertex`); the cusp case is compared by the difftest (`turn180p0_*`, `turn179p51_*` fixtures).
- **Confirmed:** yes for the cusp case (the C++ returns `TRUE`).
- **Severity:** wrong result.
- **Rust port:** Faithful.
