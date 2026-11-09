Updates: WR-95

#### WR-95 (update, 2026-10-31). Fixed in v1.0.1

Fixed in v1.0.1 (`3fd5b95`): `SmProtoEdge::CombineProtoEdge` (`source/SMLib/src/SmProtoTopology.cpp:3346`) copies `pOtherPE->m_sEdgeDefs` into `sOtherEDs` before iterating, moving all edge definitions into this proto edge rather than skipping odd-indexed definitions as elements are removed. `ProtoTopologyManager::combine_proto_edge` (`crates/solidcore/src/topology/proto_topology/p2.rs`) walks a `.to_vec()` copy unconditionally, covered by unit test `wr95_combine_proto_edge_moves_all_definitions`.
