# topology.brep_p5: the unlinked-record deferral was stale (2026-10-24)

The runner deferred any result in which a vertex use, edge use or shell stayed in the
arena unlinked (`SmBrep::~SmBrep`, `topology.brep_p4`): the 2 `isolated_op13_*` cases
(`CreateWireEdgeFromCurve`). The unlinked record is the vertex-shell vertex use of the
start vertex: `MakeEdgeVertexTopology` deletes it (`SmBrep.cpp:13869-13877`), Rust only
removes it from the vertex's list (`topology::brep::p2::remove_vertexuse_from_vertex`),
so the arena keeps it, unreachable. The guard came from 2771175cf, made on a branch
without d1ae46eab (`graph` compares only the records reachable from the B-rep, as the
native export walk emits only those); once both were on main its stated reason ("graph
lists unreachable records too") no longer held. A record wrongly still linked is reached
and still compared. Removed: 797/797 pass, 0 deferred, no library change.
`--seeds 6000` (6285/6285) is a regression check only: the generated ops (2, 12, and
3/9/11 with editing off) cannot leave an unlinked record, so only the 2 op-13 fixtures
exercise this change.

Still open, not part of this change: `SmBrep::GlueVertices` of vertices of two B-reps is
`UNPORTED(topology.brep_p5)` in the `boolean.topology_intersector_p1` runner (1 deferred
case there), because `p5::glue_vertices` takes one B-rep arena. `topology.brep_p5` is
green by its own difftest, as other units named by live stubs are (e.g. `topology.face_p2`).
