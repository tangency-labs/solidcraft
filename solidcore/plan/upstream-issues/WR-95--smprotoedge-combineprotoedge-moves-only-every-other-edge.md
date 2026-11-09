#### WR-95. `SmProtoEdge::CombineProtoEdge` moves only every other edge definition
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3332`, `SmProtoEdge::CombineProtoEdge` (def. `:3296`).
- **What happens:** The loop `for(ii=0; ii<pOtherPE->m_sEdgeDefs.GetSize(); ii++)` adds `m_sEdgeDefs[ii]` to this proto edge, which (through `AddEdgeDefinition`, the definition's owner being `pOtherPE`) removes it from `pOtherPE`'s list, then advances `ii`: the definition that moved into slot `ii` is skipped. Of `n` definitions only those at even positions move; the rest stay on `pOtherPE`, whose ends were already moved to this proto edge's.
- **Trigger:** `CombineProtoEdge` of a proto edge with two or more definitions.
- **How found:** adversarial / edge-case fixture `edge_pe_combine_every_other_unit` (unit `topology.proto_topology_p2`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
