# topology.proto_topology_p2: WR-306's degenerate test absorbs real closed curves and arcs shorter than 0.02; WR-214 alone also changes two small unions (2026-10-31)

**Units:** topology.proto_topology_p2 (`resolve_proto_edge`), boolean.topology_intersector_p1 (the absorbed-vertex fit). **Flag:** `Kernel::fixes.degenerate_proto_edge_in_one_vertex` (WR-306, #1058), the port of the fork's tangency-labs/smlib@03e7bf1. It is off by default. SolidCraft turns it on.

**What changes.** With the flag on, a proto edge from a proto vertex back to itself is degenerate below `2 * GetContainmentRadius()` when both curve ends find the same vertex definition. Its definitions are then removed, and the vertex stands for the curve. Otherwise the bound is `2 * m_dTol`, as upstream. The containment radius is `max(m_dTol, capture distance)`. `IntersectInsertRelate` sets the capture distance to an absolute 0.01, raised only to `2000 * tol` (`boolean/topology_intersector/p2.rs:1223`). So the bound is at least 0.02. A closed curve's two ends are one point, so they always find one vertex definition. Every closed intersection curve shorter than 0.02 is therefore dropped, which means every circle of radius below 0.02 / (2 pi) = 3.18e-3. The fork's case was a speck 1.4e-5 long. The bound is read from the pinned port and the fork commit, not traced in gdb. The bracket below measures it.

**Inputs and values** (Rust with only WR-306 forced on, probe batch 46g4c, against Rust with it off. On the three difftest cases the pinned C++ equals Rust with it off. The circle rows are Rust-only tests, not run on C++):

| Case | Off | WR-306 on |
| --- | --- | --- |
| `SmApiBooleanUnion` of box [0,4]^3 and a cylinder of radius 1e-3, height 1, on its top centre (`tests/wr_619_imprint_skips_collapsed_definition.rs`, `wr_619_fix_keeps_closed_edges_on_a_vertex`) | 8 faces, 15 edges, 10 vertices, 1 solid region, manifold; volume 64.00000314160312 (64 + pi * 1e-6) | 9 faces, 15 edges, 10 vertices, **2 solid regions**, manifold; volume 64.0000031. From the counts: box and cylinder left side by side on the box's top, the circle not imprinted |
| the same at radius 3e-3 (circumference 0.0188) | (8, 15, 10, 1, manifold); volume 64.0000283 | (9, 15, 10, 2, manifold); volume 64.0000283 |
| the same at radius 3.5e-3 (circumference 0.0220) | not measured (the plain run stopped at radius 3e-3's assert) | (8, 15, 10, 1, manifold); volume 64.0000385: intact |
| the same at radius 1 | (8, 15, 10, 1, manifold), 67.1416 | unchanged |
| `boolean.api_brep` `more0033` (`boolean2d` op 3 of a 0.0131 x 0.0053 plane and a disk of radius 0.0039) | pass | fail: two face areas 3.52e-5 / 5.74e-5 become 4.77e-5 / 6.99e-5 (rel. err. 0.262, 0.179); record entries 9, 7 become 7, 6 |
| `boolean.api_brep` `tess0033` (`tessellate_boundaries` of a box-sphere union about 1e-3 across) | pass, 303 points | fail: 267 points, record length 34 becomes 28 |
| `boolean.api_brep` `tess0095` (the same kind) | pass, 165 points | fail: 66 points, record length 30 becomes 28 |

The measured threshold lies between radius 3.0e-3 and 3.5e-3, which brackets the 0.02 bound on the circumference. All three generated cases are bodies smaller than the 0.02 bound. In those, the intersection curves (closed loops, or arcs whose ends lie in one 0.01 capture ball) fall under it. That those curves are the ones absorbed is likely but not traced.

**Fork or port.** The Rust port matches 03e7bf1 at every site: the containment radius, the one-definition test and its two bounds, the absorbed mark and its merge in `CombineProtoVertex`, and the vertex fit with tolerances `tol + 2 * apart` in `ImprintIntersections`. The fork's later `SmProtoTopology.cpp` commits (c9db18e, 432e589) change vertex placement, not this test. By reading, then, this is the fork's own behaviour. It has not been run on a fork build.

**Status.** WR-306 stays bit-for-bit equal to 03e7bf1. Any change would be a new opt-in flag of its own, for example a bound that does not apply to closed curves of real length. Asked on #960 whether the fork sees the same. Found while gating WR-619's `imprint_skips_collapsed_definition`, which in v2 turned WR-306 on. It no longer does. The circle test above is its guard.

**WR-214 (`Kernel::fixes.proto_edge_self_loop_on_vertex_degenerate`, #480): our own design.** WR-214 is a Rust-side opt-in deviation, not a fork fix. No fork commit exists for it, and its 2026-10-26 addendum made it opt-in. With it on, the Boolean's topology intersector clears `split_self_loops_on_vertices`, so `resolve_proto_edge` treats a self-loop proto edge whose ends a vertex definition on a vertex combined as degenerate, instead of splitting it onto that vertex as upstream does. Forced on alone (probe batch 46g4c1, everything else default), it changes two of the cases above:

| Case | Off (= C++ for the difftest cases) | WR-214 on |
| --- | --- | --- |
| `boolean.api_brep` `tess0033` | pass, 303 points, record length 34 | fail: 267 points, record length 28 |
| `boolean.api_brep` `tess0095` | pass, 165 points, record length 30 | fail: 66 points, record length 28 |
| `boolean.api_brep` `more0033` | pass | pass, unchanged |
| the radius-1e-3 circle union above | (8, 15, 10, 1, manifold) | (8, 15, 10, 1, manifold), unchanged |

Its tess0033 and tess0095 outputs match WR-306's exactly, as far as the report shows (the same point counts and record lengths). Both flags likely drop the same self-loop proto edge in those two unions; this is not traced. WR-214 has no fork-exactness constraint, so correcting it is ordinary follow-up work in this repo.
