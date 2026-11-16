# boolean.merge_p1: MergeCoincidentSurfaces wired into FixFoundFaceCoincidence (2026-10-24)

`boolean.merge_p1` reaches 0 deferred (248/248; 6000 seeds 6152/6152; main had 244 passed,
4 deferred on `SmTopologyIntersector::MergeCoincidentSurfaces`). This is the follow-up the
"Back-edges" bullet of `boolean.topology_intersector_p3` (PR #211) left open.

- **Wiring.** `SmMerge::FixFoundFaceCoincidence` is `p1::check_face_coincidence` (the
  all-edges-mated return), the merge, then `p1::finish_face_coincidence`
  (`rbModifiedTopology` from the edge counts taken before the merge). The merge needs
  `&mut` access to the adapter's own mate store, which `fix_found_face_coincidence` held
  borrowed as its map arguments, hence the split. `ManifoldKernelAdapter::fix_face_coincidence`
  (trait `ManifoldIntersector`, now with the kernels and attributes the edits notify through)
  runs `TopologyIntersector::merge_coincident_surfaces` on an `ArenaPair`, which now
  implements `IntersectorBrepP3` (modelled on the tested difftest `Two`), with two new handle
  kinds (loop use, face use). The surfaces passed are the faces' surface handles, as
  `GetSurface()`. The temporary curves the merge stores stay in the adapter's curve stores
  (handles are stable; nothing enumerates them).
- **After #721.** `IntersectorBrepP3` requires `MergeIntervalBrep` (so `ClassGeometryMut`);
  `ArenaPair` implements both. `merge_context` hands out the operand's arena, the entity,
  the handle mapping and the pair's records, attributes and context; the identity callbacks
  it borrows are held by the pair (`Identities`), built from its notification keys. A curve
  identity that caller-supplied keys refuse cannot pass through the context's infallible
  callback: the first refusal is kept and the merge fails with it (the edit in progress is
  not rolled back, as for the other adapter errors).
- **`Objs` hooks.** The private `ClassGeometry` wrapper of `face_curve_on_classify`
  returned `None` for `classification_vertex_point`, `topology_zone_tol_3d` and the topology
  hooks, which surfaced as an `SmVertex::GetTolerance` deferral inside the merge. It now
  delegates them to the pair (`vertex_face_gap` keeps the trait default, as the pair does).
  Every existing caller treats `None` as an `Unported`/invalid-input error, so this only turns
  deferrals into upstream behaviour. All-units difftest (261 units, 0 failed): no unit other
  than `boolean.merge_p1` changed (`boolean.merge_p4` 311/140/0/168, `boolean.api_brep`
  287/151/0/136, ... as on main).
- **Harness.** Op 2 compared only the primary B-rep's graph and the face lists, with the
  other operand cloned and discarded, so the merge's edits of the other B-rep were invisible.
  The shim now appends, for op 2, the faces numbered by walks after the call, both directions
  of `m_vTI`'s edge mates and the other B-rep's raw export; the runner drives the adapter
  (the context-only `SmMerge` constructor's tolerances `SM_EFF_ZERO_SQ` and 30 degrees) and
  compares `other_graph` / `other_geometry` and the mates. Not compared: notification
  records and attributes (op 2 uses fresh ones), the geometry of curves the merge creates.
- **Rectangle variants.** Op 2's generator also gives `other` another rectangle of the
  face's plane (the face's is [-2, -1, 3, 4]): overlapping, contained, enclosing, touching,
  sharing edges, and disjoint ones. Before #718 (`SmFace::FindVertexSector`,
  `topology.face_p4`) and #721 (the imprinting `MergeInterval`, `intersect.curve_class_p6`)
  each of the first five deferred 148 of 6152 cases (`--seeds 6000`, one variant at a time)
  with 0 mismatches; with both on main they all pass with 0 deferred. The touching and the
  disjoint rectangles compare the two B-reps (`with_values` 6140); for the overlapping,
  contained, enclosing and edge-sharing ones the merge returns an error status that C++
  and Rust agree on (`both_errored`, `no_values` 160 against 12). They therefore check the
  status, not the topology; a rectangle on which the merge succeeds and splits faces is
  not among them, so the post-call face numbering and the edge-mate comparison are written
  for that case but not yet exercised by a splitting merge.
