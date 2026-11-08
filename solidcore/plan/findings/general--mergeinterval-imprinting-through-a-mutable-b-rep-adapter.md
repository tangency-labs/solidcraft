# MergeInterval imprinting through a mutable B-rep adapter (2026-10-24)

`SmCurveClassification::MergeInterval` (`intersect.curve_class_p6`) is ported past its
boundary tolerances (SmCurveClass.cpp:17212-18276): the UNKNOWN-end repair from a region
or face mid (`sm_ComputeFacePointClass`), the closed/degenerate branch (`UpdatePointClass`,
`MergePointClass`, `GlueVertices` of a degenerate interval ending on another edge), and
the three edge cases: a wire edge in a region (`MakeWireEdge`), an edge in a face
(`FindVertexSector` at both ends with the stale-face recovery over the vertices' shared
faces, `MakeEdgeInFaceWithVU`, `ClassifyFaceInterval` of every interval after a split,
`RefreshTolerance` of the new edge's vertices) and the coincident edge (its tolerance
grown to the interval's gap; with `SM_USE_OLDTOL` also to the source tolerance, then
`RefreshTolerance` of its first vertex).

- **Adapter.** Upstream edits the objects behind the classification's `SmObject`
  pointers. `MergeIntervalBrep::merge_context` hands out, per topology handle, the arena,
  the entity, the map from new ids to handles and the notification state;
  `merge_interval_mut`, `merge_point_class_mut`, `merge_intervals_mut` and
  `merge_classifications_mut` edit through it. The read-only entry points run the same
  code and defer at the first edit (`SmBrep mutable owning context (...)`, unit
  `topology.brep_p1`, as part 5's read-only entry points).
- **Callers.** `TopologyIntersector::merge_curve_classes` now merges through
  `merge_classifications_mut` (`IntersectorBrepP3` requires `MergeIntervalBrep`; the
  runner's two-B-rep adapter implements it). `topology::face::p1`'s seam split
  (`split_at_seam_depth`), `topology::brep::p2`/`p7` and the healer still merge
  read-only. For `topology.face_p1`'s `torus.smb_1_f5_op9` (see "topology.face_p1
  MoveSeam split cases over handles that name topology") the split's merges need the
  `BrepSeamObjects` handles to resolve against a mutable B-rep (`topology/face/p4.rs`)
  before they can use `merge_intervals_mut`; the case still defers at
  `SmEdge::GetTolerance`, before `MergeInterval`'s edits.
- **Runner.** `intersect.curve_class_p6` gives each case a fresh copy of the box (the
  shim builds one per case) and records the box after a merge (counts, sorted vertex and
  edge tolerances, new vertex points and tolerances, new edge tolerances). The shim's
  box has editing disabled, so its edge splits (`MakeVertexSplitEdge`) and
  `MakeEdgeInFaceWithVU` fail with `SM_ERR` on both sides; 28 new `imprint_*` cases
  enable editing, pick other box objects (`sc_intersect_curve_class_p6_run_picked`) and
  place lines on the box, so a face split, a chord between two vertex loops, a
  degenerate point, a coincident edge, an inner edge split at both ends, a wire edge
  and a region mid with face ends all compare, at every flag combination. All match
  bit for bit.
- **Remaining.** `intersect.curve_class_p6`'s 20 `InsertCurveInterval` deferrals
  (`SmVertex::SetPoint`, `topology.vertex`) are `PointClassification::combine`'s
  read-only propagation of a surf/surf/surf point to a vertex, not `MergeInterval`
  (`combine_in_brep` is the mutable form). `boolean.topology_intersector_p3` keeps 15
  `topology.face_p2` and 14 `topology.vertex` deferrals reached before the merge.
