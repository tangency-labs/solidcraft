# topology.proto_topology_p2: addendum (2026-10-31): how WR-306 loses more0033's intersection: an open chord with one vertex definition

Updates: plan/findings/topology.proto_topology_p2--wr306-absorbs-small-closed-curves.md

The previous addendum (`…--addendum-2026-10-31-which-changes-are-breakages.md`) left the cause of the more0033 breakage open. A gdb trace of `resolve_proto_edge` and of WR-306's vertex fit settles it (probe batch 52, Rust only, opt-level 0, the four flag settings of `tests/probe_wr306_more0033.rs`).

**The proto edge.** more0033 is a 2D exclusive-or of a 0.013108 x 0.005336 rectangle and a disk of radius 0.0038980. It has one proto edge, pe 0: a line along the rectangle's bottom side (y = -0.0124401), from (0.0204512, -0.0124401) to (0.0276501, -0.0124401). That is the chord where the disk crosses the side, 7.1989e-3 long, with pe_tol 5e-6. Both ends lie in proto vertex 0, because they are 7.2e-3 apart, inside the 0.01 capture ball. That proto vertex has a single vertex definition, vd 0, at the chord's start. Its tolerance is 1e-5.

**Flag off (the pinned C++).** The proto edge goes from proto vertex 0 back to itself. Its 7.2e-3 length is above upstream's `2 * pe_tol` = 1e-5, so it is kept. Its ends are 7.2e-3 apart, more than `2 * pe_tol`. With only one vertex definition, `resolve_proto_edge` adds a second one at the far end (upstream's `bAddVtxDef`). The chord is then imprinted between two vertices, and the exclusive-or gives the expected faces: areas 3.523640e-5 and 5.744804e-5. WR-214 alone traces the same.

**WR-306 on.** The degenerate test runs before that split. With one vertex definition, both ends find vd 0 (`GetClosestVertexDefinition`), so the bound becomes `2 * GetContainmentRadius` = 0.02, and 7.2e-3 is below it. The proto edge is marked absorbed and all its definitions are removed. The vertex fit then runs on the two final vertices: `UpdateVertexEdgeIntervals` on B-rep 1's vertex 4 and B-rep 2's vertex 1, which lie 3.9e-18 apart, with both tolerances 1e-5. With no intersection edge left, neither face is split. The result is the two whole operands: areas 4.773347e-5 and 6.994508e-5, 7 edges, 6 vertices. Level 2 is still clean. Turning WR-214 on as well changes nothing.

The previous addendum guessed the chord's ends were on two vertex definitions. That was wrong. There was only one, because the far end was captured by the proto vertex's containment ball without getting its own definition. Upstream adds that definition only after the degenerate test, so WR-306's one-definition branch applies.

**What separates more0033 from the circle.** Both are real curves under 0.02 that WR-306 absorbs on its one-definition branch. The smallest difference is the distance between the curve's ends:

| Case | Ends apart | Length | Result with WR-306 |
| --- | --- | --- | --- |
| more0033's chord | 7.2e-3, over `2 * pe_tol`, so upstream would add a second vertex definition | 7.2e-3 | absorbed |
| the radius-1e-3 circle | 0 (closed) | 6.3e-3 | absorbed |
| the fork's own case (03e7bf1, pleated skins) | at most 1.4e-5, its length | 1.4e-5 | absorbed (intended) |

So a guard on the ends' distance would split more0033 off from the circle, but not from the fork's case (1.4e-5 is also over 1e-5). A guard that keeps the fork's case and leaves both breakages to upstream's path is the one WR-619's `speck_on_vertex` uses: every sample of the curve, ends included, within a small multiple of the tolerance of vd 0's position. For example, within `2 * max(PV tol, vd tol)` = 2e-5 here. Then the fork's 1.4e-5 speck is absorbed, while the 7.2e-3 chord (reach 7.2e-3) and the circle (reach 2e-3) are not.

**Status.** No change to WR-306, which stays bit-for-bit equal to 03e7bf1. The reach guard is a possible separate opt-in for Brett to weigh on #960. It is not implemented, and it is not checked on the fork's pleated-skins case, whose operands exist only in the fork (`capi/tests/data/pleated-skins.txt`).
