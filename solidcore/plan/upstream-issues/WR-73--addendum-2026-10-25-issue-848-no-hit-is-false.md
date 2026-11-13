#### Updates: WR-73 (2026-10-25). Issue #848, the caller stops testing the proto vertex

- **Consequence.** tangency-labs/solidcore#848: `SmTopologyIntersector::IntersectTopology`'s
  cleanup (around `SmTopologyIntersector.cpp:2194`) breaks out of its proto-edge loop when
  `RemoveCoincidentNoEdgeProtoVertex` returns `TRUE`. Because a miss (no solution, or a
  failed solve) also returns `TRUE`, an edgeless proto vertex is only ever tested against
  the first proto edge; one lying inside a later proto edge is never removed.
- **Rust port:** opt-in `Kernel::fixes.proto_vertex_miss_is_false` (default `false`, upstream's
  `TRUE`). With it, `ProtoTopologyManager::remove_coincident_no_edge_proto_vertex`
  (`crates/solidcore/src/topology/proto_topology/p1/proto.rs`) returns `false` for a miss,
  so `boolean::topology_intersector::p2`'s loop goes on to the remaining proto edges. A
  failed solve stays `false` too (the caller already reads an error as `FALSE`). The issue's
  further direction, splitting a proto edge at a proto vertex inside it, is not ported.
- **Test:** `issue_848_no_hit_is_false` in `crates/solidcore/src/topology/proto_topology/p1.rs`.
- **Difftest:** with the default, `topology.proto_topology_p1` and
  `boolean.topology_intersector_p2` compare as before.
- Fixes tangency-labs/solidcore#848.
