# sweep.topology_sweep: whole-B-rep DoSweep of non-manifold bodies (2026-10-27)

**Unit:** `sweep.topology_sweep`. New fixtures `fixtures/sweep.topology_sweep/nonmanifold_body.json`
(34 cases) run the whole-B-rep `SmTopologySweep::DoSweep` (all six target arrays NULL) on
bodies built by the same kernel calls on both sides (`sc_sweep_topology_sweep_body`, Rust
`body_rust` in a `SweepStore`): the prog_test `my_create_test_brep` face
(`smSweepTest.cpp`; its `my_test_tsurf_creation` face with two inner loops, plus a vertex
loop, an edge loop, a wire edge and a shell vertex), a box with a wire edge and a shell
vertex, a lone wire, a lone shell vertex, an empty B-rep, the box alone and the face
alone. The record adds the wire-edge and shell-vertex counts and every face and edge
`SM_AI_TAG` (as sorted multisets) to the `whole` record.

1. **Fixed: `CopyEdges` in a `SweepStore` copied nothing.** The `SweepAccess::copy_edges`
   default called the plain `topology::brep::p6::copy_edges`, which lends the kernel
   context's notify callbacks to its own edit; inside `with_breps_mut` they are already
   lent, so every call failed with `InvalidInput("... lent to an edit in progress (nested
   edit)")`, and the whole-B-rep step 2a ignores `CopyEdges`'s status (as upstream), so
   the wire B-rep stayed empty: every body with a wire edge lost it (and its swept face)
   without an error. The default now calls the new `p6::copy_edges_with_context`, which
   makes each wire edge (`CreateWireEdgeFromCurve`) and reports each copy through the
   lent environment, as `copy_vertices_with_context` already did for `CopyVertices`.
   Before: 10/28 of the first fixture set passed; after: all but the entries below.
   Open (Rust-only hazard): a Rust plumbing refusal like this one is swallowed by
   `ignore_status` where upstream ignores a native status.
2. **Undefined in C++, refused by the FFI guard** (`body_undefined`; cases
   `nm_wire_linear_z_two_reps_ub269`, `nm_box_linear_z_tagging_ub270`, outcome
   `rejected`):
   - UB-269: a repetition without wire edges or shell vertices after one with them sweeps
     and unions `pWireBrep`, which the previous repetition's `ManifoldBoolean` deleted.
     The lone wire swept twice crashes the C++ in 200/200 fresh processes. The Rust port
     clears the handle after the union (already noted in
     `sweep.topology_sweep--pr-222-partial.md`, which had no native reproducer).
   - UB-270: with tagging, the whole-B-rep `TagProfile(NULL, &sAllEdges, ...)` reads the
     wire edges after `RemoveAllTopology` deleted them: box with wire, tagging on, 129/200
     crashes and 71/200 `SM_ERR_NULL_POINTER`.
   Without wires, tagging is compared: the box alone and the face alone return
   `SM_ERR_NULL_POINTER` on both sides (`DoSweepEdgesHigher`'s `NER` on an untagged
   silhouette edge, since the whole-B-rep `TagProfile` tags only wire edges).
3. **Parked, ND-491** (`nm_box_only_linear_z_stitch`, the 4x3x2 box alone, translation
   (0,0,10), stitching on): C++ succeeds (1 region, 14 faces, 28 edges, 16 vertices) in
   394/400 heap-varied fresh processes and returns `SM_ERR` in 6/400; Rust returns
   `SM_ERR`, one of the observed C++ results (the stitch callback's
   `SelectTopologyToKeep` refusing a pair, which depends on the silhouette order).
4. **Known mismatch, not parked** (`nm_box_linear_z_stitch`, the same box with a wire
   edge and a shell vertex, same sweep): C++ succeeds (3 regions, 15 faces, 33 edges,
   22 vertices) in 400/400 heap-varied processes; Rust returns `SM_ERR` (1001) before
   the wire sweep, i.e. in the stitch of the box part, which is the same B-rep as in 3
   at that point (the wire edge and shell vertex are set aside first). Suspected cause:
   the same ND-491 silhouette-order dependence with a C++ heap layout that never
   produced the failing order for this body; not demonstrated, so it stays a known
   mismatch.
5. **Known mismatch, not parked** (`nm_test_rotation_neg_y_36`: the prog_test body
   rotated 36 degrees about (0,-1,0) through (-10,0,0), eps 1e-4, stitch and merge off):
   C++ returns `SM_ERR` (1001) in 400/400 heap-varied processes; Rust succeeds (1
   region, 11 shells, 15 faces, 21 loops, 66 edges, 63 vertices). The same rotation of
   the face alone (`nm_face_only_rotation_neg_y_36`) and the prog_test body rotated 30
   degrees about (0,1,0) (`nm_test_rotation_y_30`) pass; 30 degrees about (0,-1,0) and
   36 about (0,1,0) fail the same way. A C++ probe of the failed B-rep (14 faces, 3 wire
   edges, 2 shell vertices after the failure, against 14 faces before the union on the
   Rust side) places the failure in the final `SmMerge::ManifoldBoolean(SM_BO_UNION)` of
   the swept body with the swept wire B-rep (the wire starts at a face vertex, so its
   swept face meets the swept body along that vertex's arc), which Rust completes.
   Suspected cause: a divergence in the union (`boolean.merge_*`) for a sheet touching
   the body along an edge; not localized further.
