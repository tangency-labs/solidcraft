#### UB-116. `SmProtoEdge::AddEdgeDefinition` compares uninitialised tangents
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3189` and `:3190`, `SmProtoEdge::AddEdgeDefinition(SmEdgeDefinition*)` (def. `:3173`).
- **What happens:** The statuses of both `EvaluateMid(NULL, &tan)` calls are ignored; `SmProtoEdge::Evaluate` / `SmEdgeDefinition::Evaluate` return before writing the tangent when an evaluation fails (and dereference `NULL` for a definition without 3d curve), so the reversal decision reads an uninitialised `SmVector3d`.
- **Trigger:** adding a definition to a non-empty proto edge whose (or whose definitions') curve fails to evaluate at the proto edge's mid parameter, or has no curve.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim evaluates both first and refuses the call when either fails).
- **Severity:** UB.
- **Rust port:** `SolidError::InvalidInput`.
