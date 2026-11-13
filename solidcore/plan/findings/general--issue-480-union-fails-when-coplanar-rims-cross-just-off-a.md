# Issue #480: union fails when coplanar rims cross just off a vertex (topology.proto_topology_p2, 2026-10-22)

`SmApiBooleanUnion` of a ring and a half-ring lip whose r5 cylinders are offset by e
fails (`SM_ERR`) for 4e-5 < e <= 2e-4 when the ring's circles have their seam vertex at
(0,15,2), next to where the two rims on the coplanar faces cross ((e/2, 15, 2)). This was
reproduced and traced on the C++ reference build; the cause is in the proto topology
(plan/upstream-issues.md WR-214), not in the coplanar face merge or the curve/curve
intersector as first suspected.

- `CanCombineVertexDefinitions` merges the crossing and the vertex into one proto
  vertex (they are e/2 <= `5 * (1e-5 + 1e-5)` = 1e-4 apart), keeping the definition on
  the vertex. `SmProtoEdge::Resolve` then re-splits the short intersection curve
  between them (ends e/2 > `2 * m_dTol` = 2e-5 apart) by copying that definition, so
  both ends are the ring's one vertex while the lip gets a short edge between two
  vertices. `MakeFaceuse` fails at `SmMerge.cpp:7071`. Window: 2e-5 < e/2 <= 1e-4.
- With the seam at (5,10) (`SmApiCreateCylinder` default) every offset succeeds; the
  failure needs the vertex.
- The reporter's e = 1e-3 failure was not reproduced with this construction; their
  operands come from SolidCraft's own sketch/extrude path and may place vertices
  differently.

Fix (documented deviation, `topology::proto_topology::p2`): with
`ProtoTopologyManager::set_split_self_loops_on_vertices(false)`, `resolve_proto_edge`
does not split a self-loop proto edge whose definition is on a vertex in either B-rep;
it drops the proto edge's definitions, as upstream does when the curve is shorter than
`2 * m_dTol`. The Boolean's topology intersector (`boolean::topology_intersector::p2`,
`IntersectInsertRelate`, the only caller of `resolve_proto_edge`) sets it; the default
keeps upstream's split. The same change in a private C++ copy makes all failing offsets
a manifold solid of 354.99989 mm^3 with the topology of the snapped e = 3e-5 case,
leaves every other offset unchanged, and is never taken in `SM_API_test` or
`prog_test` (both pass).

Why a setting: 7 of the 1533 generated `topology.proto_topology_p2` difftest cases reach
this branch (with the change unconditional: 1455 pass, 7 fail on `values.out.len`, e.g.
gen0405 C++ 373 vs Rust 84, against 1462 pass at the parent commit). With the default
they compare the faithful port against the unpatched C++ as before, and no case is
dropped. `boolean.topology_intersector_p1` (830 cases, 797 pass, 33 deferred) is
unchanged with the setting on.

Validation limits: the union cannot run end to end in Rust yet (primitive creation,
`SmFace::FaceIntersect`, `topology.brep_p4` deletes and the `MergeCoincidentSurfaces`
wiring are unported). Unit tests cover the new branch (definition on a vertex in
either B-rep: dropped), the default (split attempted) and the unchanged case off a
vertex.
