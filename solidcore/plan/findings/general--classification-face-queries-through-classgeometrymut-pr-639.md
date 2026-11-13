# classification face queries through `ClassGeometryMut` (PR #639, 2026-10-23)

- **What changed.** `query::face::p2::WithMadeTrimCurves` forwards the topology hooks
  (`topology_entity`, `topology_handle(_in)`, `brep_id`, `owner_entity`, the intersector
  mate) to the caller's geometry, with the face's B-rep resolved in the classification's
  private snapshot, and implements `ClassGeometryMut` on that snapshot.
  `CurveClassification::classify_face_interval_mut` (`intersect::curve_class::p7`) runs
  `SmFace::EdgeCurveOnClassify` and `Point3DClassify` on the B-rep the face handle
  resolves to, so their UV trims and tolerance changes stay for the rest of the
  classification (upstream order). `CurveOnClassify` uses it. No new B-rep copy is made.
  `FindVertexSector` and `PointClassify` stay deferred (below).
- **#614's copy path is kept as a deviation.** The read-only `classify_face_interval`
  still runs `EdgeCurveOnClassify` / `Point3DClassify` on a clone of the face's B-rep and
  discards what they add (UV trims, caches, tolerance growth); upstream's `const`
  members keep them. Recorded in the `p7` Deviations. Callers should move to the
  mutable entry point.
- **Resolved: seam EDGE (Rust, edge parameter 1.0) vs VERTEX (C++)** (the latent
  disagreement recorded under the query.face_p1 hooks entry above). Cause: the
  `heal.face_props` runner built its one-face B-rep without the down face use's side
  (`heal_face_props::add_down_side`, which `SmBrepConstructor` builds and the
  curve_class_p4 runner already adds). With a single edge use per edge,
  `SmEdge::GetOtherVertex` (edge-end snap of `sm_InsertSolutions`) returned the start
  vertex itself, 3.11 away from the edge end in gen0009, so the hit stayed EDGE. The
  runner now adds the down side; gen0009's U seam intervals match C++ exactly.
- **Fixed on the way (exposed once the classifications ran):**
  - `SweepEdgeHigh` (`sweep::topology_sweep::engine`) failed with
    `SM_ERR_NULL_POINTER` when `MakeFaceWithCurves` dropped the far-end or a vertex
    sweep curve (degenerate, or coincident with an earlier edge). Upstream tests those
    curves with `s3DCurves.FindElement` (the slot is nulled) and treats them as stale;
    the port now does too (primitive_creation_p2 gen0094, gen0179, sweep.api_brep
    review_rot_angle0). Upstream's original-edge curve has no such test (it reads a
    deleted curve's owner: use after free) and the port still errors there.
  - `Surface::evaluate_normal_at_singularity` returned `InvalidInput` for a zero normal
    sum; upstream's `SER(Unitize())` returns the `SM_ERR_INVALID_INPUT` status, which
    `HasSeamProblem` ignores. It now returns that status (topology.brep_p10
    read_Fillet_bottle, read_Fillet_cyl_cyl_shell, read_Fillet_fillet0227 then defer
    on `GetLoopCrossings` instead of differing in the last bits).
- **Why `FindVertexSector` / `PointClassify` stay deferred.** Wired on the mutable path
  (the code is on branch `wire/face-sector-classify` of brett19's fork), they turn about
  950 deferrals into passes but expose two kinds of disagreement:
  1. **Trimmed surface cache history (tolerance growth).** 15 cases (heal.face_props
     gen0009, gen0384, gen0616, gen0646, gen0703, gen0790, gen1066, gen1312, gen1323;
     curve_class_p4 gen1570, gen1622, gen1841, gen1876, gen1968, gen2061; all
     `SmSurfOfRevolution`). In gen0009 the first `FindVertexSector` of the U seam
     reaches `SmEdgeuse::EvaluateBinormal` -> `smsurf_GetSurfaceCache`, which in Rust
     always builds the face's trimmed cache with the face: `CreateUVTrimCurves` ->
     `SetUVTrimCurve` raises every edge tolerance (edge 0: 1e-5 -> 5.136, edges far off
     the torus). The V seam then intersects edges with those tolerances and finds hits
     C++ does not. A C++ probe in the shim showed edge tolerances stay 1e-5 and no edge
     use gets a UV trim curve, while the surface holds an `SmTrimSrfCache` with
     `m_bHaveTSurfaceCache = 0`, `m_bFaceContainmentDone = 0`: upstream reused a
     trimmed cache built earlier without the face (owner cleared). Running the queries
     on a throwaway copy instead (the #614 model) leaves 10 disagreements, so neither
     model matches. The mutable path already defers a face query that raises a vertex
     or edge tolerance (`smsurf_GetSurfaceCache (trimmed surface cache history:
     tolerance growth)`, `surface.surface_p2`); with both queries wired that defers
     54 heal.face_props, 180 curve_class_p4 and 27 query.face_p3 cases.
  2. **`IntersectTSBound` without the face's trimmed cache.** topology.face_p2
     extra01204 (`CreateCurvesByTrimming`, a UV curve): C++ classifies
     `[0, 0.7232]` FACE, end VERTEX; Rust inserts vertex hits at 0 (vertex 2) and at
     0.0435 (vertex 0), then `ClassifyFaceInterval` resolves the conflicting
     vertex sectors by deviation. `FindVertexSector` itself agrees (a C++ probe gives
     the same sector answers for vertices 0 and 3). Upstream's `IntersectTSBound`
     calls `smsurf_GetSurfaceCache(pSurface)` itself, so it nearly always takes the
     cached leaf-node traversal (only vertices in leaf nodes the curve meets); the
     port's `query::face::p2::intersect_ts_bound` uses only the caller's
     `CurveOnClassifyObjects::trim_cache` and otherwise walks every loop use.
- **Follow-up needed for them:** build (or reuse) the face surface's trimmed cache in
  `intersect_ts_bound` as `smsurf_GetSurfaceCache` does, with a `CacheFace` adapter
  that names the cache's vertex and edge uses in the caller's handles; then model
  per-surface object caches (a face-owned trimmed cache, built with or without the
  face, that later `smsurf_GetSurfaceCache` / `GetObjectCache` calls reuse) so the
  cache-history deferral can go.
- **Left out: `topology::face::p2::UvLineObjects` hooks.** Giving the UV-line adapter
  `topology_entity` / `topology_handle` makes topology.face_p4 (80 `SmEdge::GetCurve`
  deferrals) and topology.topology_traverser (1) green but turns topology.face_p2
  deferrals into 6 failures in `CalculateAnInternalPoint` (internal_bss_tiny,
  extra00702, extra00744, extra00806, extra01154, extra01204). In internal_bss_tiny
  the second UV line (v = 1/21 of a 1e-4 face, within 2e-5 of edge 1) classifies C++
  VERTEX-FACE-VERTEX but Rust VERTEX-EDGE-VERTEX (a coincidence range with edge 1).
  The C++ face surface has a trimmed cache with face containment done by then, so
  C++'s `IntersectTSBound` takes the cached traversal: the same gap as item 2 above.
- **Not wired: `FindVertexSector` in `boolean::topology_intersector::p1`
  (`HandleFaceEdge::sector`).** Its next call, `SmBrep::MakeEdgeInFaceWithVU`
  (`insert`), is stubbed too and `IntersectorBrep` gives no mutable B-rep; wiring the
  sector alone would only move the deferral. Needs an `IntersectorBrep` hook to the
  mutable B-rep (both backends: `merge::p4::ArenaPair` and the ti_p1 runner).
