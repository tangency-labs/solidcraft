#### WR-73. `RemoveCoincidentNoEdgeProtoVertex` returns `SM_SUCCESS` as TRUE
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmProtoTopology.cpp:6925` `SmProtoTopologyManager::RemoveCoincidentNoEdgeProtoVertex`: `if(eStat != SM_SUCCESS || sSols.GetSize() == 0) { return SM_SUCCESS; }` in a function returning `SmBoolean`.
- **What happens:** When the point solve fails or the proto vertex is not on the proto edge's curve, the function reports `TRUE` ("coincident and removed"), although nothing was removed.
- **Trigger:** A proto vertex off the first edge definition's curve.
- **How found:** generated difftest case (`topology.proto_topology_p1`).
- **Confirmed:** yes (the C++ run returns 1 with the proto vertex still listed).
- **Severity:** wrong result.
- **Rust port:** Same result (`true`), for parity.
