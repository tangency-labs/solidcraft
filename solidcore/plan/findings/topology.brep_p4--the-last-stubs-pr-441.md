# topology.brep_p4: the last stubs (PR #441)

Follow-up on PR #441 (oursland/solidcore#1, on top of 55b9202), 2026-10-22.

- **The 36 heal.stitch_p2 differences behind the glue stub** (section "StitchAndOrient's
  second pass, and the glue stub that hides 36 heal.stitch_p2 differences" above). They are
  not in the shell-vertex glue: `GlueEdgesGeneral` (`topology.brep_p7`) did not take the glued
  edge off `Brep::edge_list` (upstream `m_pEdgeListHead->Remove(pEdgeToDelete)`; p7's
  deviation note predated the separate list), so each `StitchIntoShell` case counted the
  orphan edge (C++ 7 edges, Rust 8; the orphan has no edge uses). With that fixed,
  `GlueVertices`' `m_pVertexListHead->Remove` is wired (`Brep::remove_from_vertex_list`):
  heal.stitch_p2 505/556 pass, 0 failed, 51 deferred (was 368 pass, 188 deferred).
  A diagnostic run that logged every refused notify lend across `difftest --green` (255
  units) found no nested lend besides the one fixed in StitchAndOrient.
- **Wired.** `StitchFaces` / `ShrinkGeometry` / `DeleteFace` in `primitive_creation` p1/p2's
  access; `StitchFaces` in the fillet executive (a required `ExecutiveAccess` method, with
  `StitchAttempt::from_stitch`, implemented by the p2/p3 difftest adapters). The
  "opaque object handles" sites above are hooks now: `IntersectorBrep::brep_delete_edge` /
  `brep_delete_vertex` (topology_intersector p2 `DeleteFaces`, p3 `MergeCurveClasses`) and
  `BrepGlue::delete_edge` (proto_topology p3), implemented by `merge::p4::ArenaPair` and the
  difftest adapters. No difftest case reaches the p2/p3/proto_topology deletions yet.
- **Still stubs (`justified:` with this section).**
  - `blend/fillet_corner/p1.rs` `pseudo_brep_delete_edge`: the corner geometry pass holds no
    `Kernel`, and the pseudo B-rep's fillet records (its edge list, the `SmFilletEdge` /
    `SmFilletVertex` members) would have to drop whatever `DeleteEdge` deletes. No difftest
    case reaches it.
  - `offset/offset_executive/p2.rs` `do_solid_offset` (cpp:608 `ShrinkGeometry`): the stub
    also marks where this unit's own unported continuation (cpp:648-1051, BuildOffsetOfEdgeuses
    through MakeManifold) begins. Wiring the shrink alone leaves that remainder without a
    stop; it belongs with porting the continuation (3 deferred cases).
- **Harness, not a stub.** `topology.brep_p5`'s 2 cases deferred as `SmBrep::~SmBrep (stable
  owning record erasure)` come from its runner, which defers whenever an edit leaves
  unlinked records in the arena (`has_unlinked_records`).
