Updates: WR-579

#### WR-579 (update, 2026-11-17). `proto_edge_splits_at_captured_vertices` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, zero-row batch). `ProtoTopologyManager::resolve_proto_edge` (`topology/proto_topology/p2.rs`) always splits a proto edge from a proto vertex back to itself, whose curve ends find two of its vertex definitions, at each other definition of that proto vertex lying on the curve (`split_at_captured_vertex_definitions`), then resolves the proto vertex, as the fork's tangency-labs/smlib@9eec159 does. Removed: the field `UpstreamFixes::proto_edge_splits_at_captured_vertices`, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260`'s list, the test settings in `issue_1945_fork_1207_step4.rs`, `issue_2020_offset_mitred_below_depth.rs` and `wr650_tabbed_plate_mitred_rounds.rs`, and the flag-off halves of `wr579_near_coincident_tube_ends_join` and `wr580_tube_joint_no_coincident_edges` (each keeps every assertion on the default kernel).

- **Results:** no goldens row moves. All 283 units were run with the split forced on against main's expectations: 283 passed, 0 rows changed. SolidCraft's preset and the kernel rig (`fixes.json`) already ran it.
- **Tests:** since WR-533's split of a coincident definition across a split point became the only path, the #1517 union joins with or without this split, so no known input tells the two apart. `wr579_near_coincident_tube_ends_join` and `wr580_tube_joint_no_coincident_edges` still run the code on the default kernel.
