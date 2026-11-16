Updates: WR-73

#### WR-73 (update, 2026-11-16). `proto_vertex_miss_is_false` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch topology-2). `RemoveCoincidentNoEdgeProtoVertex` (`topology/proto_topology/p1/proto.rs`) answers `false` when its point solve finds nothing, and a proto vertex with proto edges is `false` without the solve; upstream's `SM_SUCCESS` (read as `true`) on a miss is gone. SolidCraft's preset already had the flag on. Removed: the field `UpstreamFixes::proto_vertex_miss_is_false`, SolidCraft's preset line, its entry in `fixes.json` and the `issue_2260` list, and every test setting of it.

- **Results:** 61 goldens rows move on the library's default kernel (`topology.proto_topology_p1` 27, `topology.proto_topology_p3` 34). In each the stored answer of one call goes from 1 to 0 (of two calls in two rows), and nothing else in the row moves. The goldens test `topology_proto_topology_p1::tests::every_case_meets_the_wr73_and_transform_certificates` replays every case of both units and asserts at each call that a `true` answer leaves the proto vertex unlisted, which upstream's answer on a miss breaks.
- **Tests:** `p1.rs::tests::proto_vertices_combine_and_remove` now expects `false` for the vertex 5 off the line (a miss); `issue_848_no_hit_is_false` drops its upstream half; `tests/wr73_proto_vertex_miss_is_false.rs` keeps its default-kernel run (`..._lip_union_builds`).
