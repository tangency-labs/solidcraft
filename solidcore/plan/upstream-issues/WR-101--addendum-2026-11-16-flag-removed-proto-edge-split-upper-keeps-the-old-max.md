Updates: WR-101

#### WR-101 (update, 2026-11-16). `proto_edge_split_upper_keeps_the_old_max` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch topology-2). `SmProtoEdge::Split` (`topology/proto_topology/p3.rs`) gives the upper child `[dT, old_max]`; upstream's `[dT, dT]`, read after clamping, is gone. SolidCraft's preset already had the flag on. Removed: the field `UpstreamFixes::proto_edge_split_upper_keeps_the_old_max`, SolidCraft's preset line, its entry in `fixes.json` and the `issue_2260` list, and every test setting of it.

- **Results:** no goldens row moves. On main, since #17399 and #16608, a proto edge splits only where an edge definition splits too, so the upper child already gets `[dT, old_max]`. (Before those merged, three `topology.proto_topology_p3` rows moved: gen0894, gen2209 and gen2929.) The unit test `proto_edge_split_upper_interval_keeps_the_old_max` (`topology/proto_topology/p2.rs`) asserts the children `[old_min, dT]` and `[dT, old_max]`.
- **Tests:** `p2.rs::tests::proto_edge_split_upper_interval_keeps_the_old_max` keeps only its fixed half.
