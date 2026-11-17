Updates: WR-410

#### WR-410 (update, 2026-11-17). The best crossing is the default; the flag is retired

`Kernel::fixes.boolean_vertex_by_best_crossing` is removed (tangency-labs/solidcraft#14246, carrying
tangency-labs/solidcraft#15004 by @bherbruck). `FindOrCreateProtoEdge` (`topology/proto_topology/p3.rs`) now always
passes each end's proto vertex to `add_crossing` with the angle at which the face pair's curve crosses the edge, so
a vertex definition takes the position of its steepest crossing. The upstream behaviour (the first crossing places
the vertex) is no longer reachable in the Rust kernel; the C++ defect is unchanged in v1.0.1, as recorded above.
SolidCraft's preset already had it on, so SolidCraft's results do not change.

Effect on the default kernel (goldens blessed on solidcraft main 49efdea29 with the change): two rows move, both in
`offset.offset_executive_p2`, with their counts and status unchanged.
- `extrude_holes1_face0_inset24_7_rounded`: vertex bounds lo.x, lo.y by about 1e-7, lo.z by 2.8e-13, hi.y by 1.0e-6,
  hi.z by 9.8e-7.
- `shell_file_problemShell10_face1_round`: hi.z by 1.03e-5 (a different vertex is now highest).

Every moved vertex is closer to the surfaces of the faces it bounds (largest gap 2.75e-7 -> 2.1e-12, 9.20e-7 ->
3.66e-7, 3.3e-13 -> 1.5e-14, 9.73e-6 -> 2.9e-14; one unchanged at 7.93e-7 with its edge gap 4.70e-6 -> 2.85e-6):
`crates/solidcore/tests/wr_410_best_crossing_offset_vertices.rs`, which fails with the change reverted.
`boolean.api_brep` does not move (the 2026-11-10 count of 7 rows predates the strip identity of #15718).

Outside the goldens, a sweep of the WR-614 strip unions (slopes 0.02 to 0.5, d from 5e-6 to 1e-4) changes one
result: at slope 0.2 and d = 2e-5 exactly (the intersection tolerance), the union keeps the strip (11 faces, the
strip's closed-form volume) and fails Level 2's SmFace test 33 (coincident edges), where the first crossing collapsed
the strip (10 faces, Level 2 clean). The same failure already occurs with the first crossing at slopes 0.1 and 0.3
at that d; it is the collapse-boundary defect of tangency-labs/solidcraft#16351, not a new one.
