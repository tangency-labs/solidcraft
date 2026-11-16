Updates: WR-101

#### WR-101 (update, 2026-11-16). `proto_edge_split_upper_keeps_the_old_max` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch topology-2). `SmProtoEdge::Split` (`topology/proto_topology/p3.rs`) gives the upper child `[dT, old_max]`; upstream's `[dT, dT]`, read after clamping, is gone. SolidCraft's preset already had the flag on. Removed: the field `UpstreamFixes::proto_edge_split_upper_keeps_the_old_max`, SolidCraft's preset line, its entry in `fixes.json` and the `issue_2260` list, and every test setting of it.

- **Results:** 3 goldens rows of `topology.proto_topology_p3` move (gen0894, gen2209, gen2929), downstream of a split's upper interval. The goldens test `topology_proto_topology_p1::tests::every_case_meets_the_wr73_and_transform_certificates` replays every case of `proto_topology_p1` and `p3` and asserts at each split that the children are `[old_min, t]` and `[t, old_max]`.
- **Tests:** `p2.rs::tests::proto_edge_split_upper_interval_keeps_the_old_max` keeps only its fixed half.
