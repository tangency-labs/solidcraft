#### WR-101. `SmProtoEdge::Split` gives the upper child proto edge the empty interval `[dT, dT]`
- **Severity:** Wrong result
- **Area:** Numerical integration
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3037`-`3038` (`SmProtoEdge::Split`).
- **What happens:** After `this->m_pXSectInterval.SetMinMax(min, dT)` the next line sets the new upper proto edge to `SetMinMax(dT, m_pXSectInterval.GetMax())`, which reads the maximum of `this` that was just changed to `dT`. The upper child gets `[dT, dT]` instead of `[dT, oldMax]` (its edge definitions are split correctly, by `SmEdgeDefinition::Split`).
- **Trigger:** Any interior split of a proto edge (from `SplitIntersectingProtoEdges`).
- **How found:** read during porting.
- **Confirmed:** no (the split needs `topology.proto_topology_p2`, so the difftest cases that reach it are deferred).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`ProtoTopologyManager::proto_edge_split`).
