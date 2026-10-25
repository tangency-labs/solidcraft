#### UB-115. `CreateVertexDefinition` / `CreateProtoVertex` use an unset position and gap
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3754` `SmProtoTopologyManager::CreateVertexDefinition`, `:3961` `SmProtoTopologyManager::CreateProtoVertex` (the ignored `sm_FindPositionFromPCs` calls; `sm_FindPositionFromPCs` at `:84`).
- **What happens:** Both ignore the status of `sm_FindPositionFromPCs`. On its error returns (neither classification has parameters, an object point cannot be evaluated) the local `SmPoint3d` (`SmVector3d() = default`) and, on the early returns, the deviation `double` are never written, and are then used as the definition's position and gap / the proto vertex's position and tolerance.
- **Trigger:** two classifications without parameters, or one on a region (`FindObjPoint3d` of `SM_PC_REGION` is `SM_ERR`), e.g. `CreateProtoVertex` of a region and a face classification.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim replays `sm_FindPositionFromPCs` on copies and refuses the call when it fails).
- **Severity:** UB (uninitialised read).
- **Rust port:** `SolidError::InvalidInput`.
