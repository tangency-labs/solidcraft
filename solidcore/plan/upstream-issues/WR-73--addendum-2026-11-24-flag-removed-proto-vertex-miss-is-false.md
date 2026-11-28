Updates: WR-73 (`WR-73--removecoincidentnoedgeprotovertex-returns-sm-success-as-true.md`)

#### WR-73 (update, 2026-11-24). `proto_vertex_miss_is_false` is removed

`RemoveCoincidentNoEdgeProtoVertex` now always answers whether it removed the proto vertex: a miss answers `false`, so the boolean's proto-vertex cleanup goes on to the remaining proto edges. Upstream's `SM_SUCCESS` read as `true` stopped it at the first proto edge. The fix is now unconditional and the flag is gone. Removed: the field `UpstreamFixes::proto_vertex_miss_is_false`, its page `kernel/fixes/proto_vertex_miss_is_false.md` and every test setting of it (tangency-labs/solidcraft#18927, together with WR-662's `ssi_proto_topology_statuses_pass_on`).

- **Results:** the value-only rows of `topology.proto_topology_p1` and `_p3` whose script calls the removal on a miss now record `false`. No real-model golden moves through this flag alone; the counts are in the WR-662 addendum of this date, which lists every moved row.
- **Tests:** `goldens` `units::topology_proto_topology_p1::status_proof::status_proof_replays` replays every case of the three proto-topology units and asserts, for every removal call, that its answer equals "the manager listed the proto vertex before the call and not after it" (PROVEN for every call the units make). `tests/wr73_proto_vertex_miss_is_false.rs` (the public union whose cleanup reaches the later proto edges) no longer sets a flag.
