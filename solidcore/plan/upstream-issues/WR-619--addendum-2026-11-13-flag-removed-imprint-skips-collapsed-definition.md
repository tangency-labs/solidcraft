Updates: WR-619

#### WR-619 (update, 2026-11-13). `imprint_skips_collapsed_definition` is removed

The fix is the default and its flag is gone, together with WR-614's `boolean_coincident_strip_drops_crossing`: an
edge definition that would be a closed edge on one vertex in each B-rep along a speck within that vertex's tolerance,
beside a sibling imprinted between distinct vertices, makes no edge (`topology/proto_topology/p2.rs`,
`collapsed_beside_sibling`), and `ImprintIntersections` grows the vertex tolerance over the speck's span
(`boolean/topology_intersector/p1.rs`). SolidCraft's preset and the kernel rig's `fixes.json` already had it on. Results
and goldens: the WR-614 addendum of 2026-11-13.
