#### Updates: WR-214 (2026-10-26). The fix becomes opt-in; the default is upstream's

- **Change (following tangency-labs/solidcore#1077, as WR-255 / WR-259):** the Boolean's
  topology intersector (`TopologyIntersector::intersect_insert_relate`,
  `crates/solidcore/src/boolean/topology_intersector/p2.rs`) no longer forces
  `ProtoTopologyManager::set_split_self_loops_on_vertices(false)` on every Boolean. It
  sets it from the opt-in `Kernel::fixes.proto_edge_self_loop_on_vertex_degenerate`
  (`UpstreamFixes`, off by default), so by default `resolve_proto_edge` splits the
  self-loop proto edge onto the vertex as `SmProtoEdge::Resolve` does, and the union of the
  trigger fails in `SmMerge::MakeFaceuse` with `SM_ERR` (1001), as the C++ build does.
- **Correction:** the record's "The full union does not run in Rust yet" no longer holds.
  The trigger runs in Rust: by default, 5e-5, 7e-5, 1e-4 and 1.5e-4 return
  `make_faceuse: SM_ERR`. With the field on they give a manifold solid of the analytic
  volume.
- **Tests:** `issue_480_ring_lip_union_with_the_seam_at_the_crossing` (field on) and
  `issue_480_ring_lip_union_with_the_seam_at_the_crossing_fails_by_default`, in
  `crates/solidcore/tests/issue_480_ring_lip_union.rs`.
- **Difftest:** the trigger is not a `boolean.api_brep` case. Its ring (cylinder r5 minus a
  coaxial cylinder r3) already reaches UB-175 (`~SmProtoTopologyManager` reads a freed
  proto edge), so the runner would reject the input. The unit
  `topology.proto_topology_p2` still compares the manager at its upstream default.
- **Callers** that relied on the previous default (e.g. SolidCraft, #480) must set the
  field.
