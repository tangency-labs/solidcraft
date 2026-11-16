#### UB-117. `UpdateEdges` dereferences its optional calling vertex definition
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:5074`, `SmProtoTopologyManager::UpdateEdges` (def. `:4975`).
- **What happens:** `pCallingVD` is documented "Optional ..., NULL to ignore, default:[NULL]" (and compared against at `:5013`), but once an edge was split in two `pCallingVD->GetProtoVertex()` dereferences it unconditionally.
- **Trigger:** `UpdateEdges(iWhich, pOldEdge, {pOldEdge, pNewEdge})` without a calling definition.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim refuses it).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput`.
