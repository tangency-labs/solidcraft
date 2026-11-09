# query.face_p1 hooks: what still blocks the remaining deferrals (claude/face-p1-hooks, 2026-10-23)

- **Blocker (query.face_p2 code).** `SmFace::EdgeCurveOnClassify` / `Point3DClassify` are now
  dispatched from `ClassifyFaceInterval` (`intersect::curve_class::p7`) on the B-rep that
  `ClassGeometry::topology_entity` resolves the face handle to. Every `CurveOnClassify` path
  (`query::face::p2::curve_on_classify`) wraps the caller's objects in `WithMadeTrimCurves`,
  which does not forward `topology_entity` / `topology_handle(_in)`, so the face handle stays
  unresolved and the case keeps its `query.face_p1` deferral. This covers all remaining
  `query.face_p1` deferrals on main: intersect.curve_class_p4 97, topology.brep_p3 38,
  heal.face_props 27, topology.face_p2 19, query.face_p2 4, query.face_p4 1, topology.brep_p1 1,
  and blend.fillet_solver_p5 3 (its runner goes through the same wrapper; not checked on the
  Mac, where the unit fails at main too). The forwarding exists on `codex/primitive-p2-production`
  (commit 4526f4aa0).
- **Latent disagreement (shows up once that forwarding lands).** Mac run of this branch plus
  4526f4aa0's `p2.rs` hunk: heal.face_props 8 failures (gen0009, gen0205, gen0492, gen0616,
  gen0620, gen1053, gen1315, gen1483) and intersect.curve_class_p4 2 (gen1841, gen1968). All
  are on `SmSurfOfRevolution` faces (torus/cylinder kinds). In gen0009 the seam-U classification
  of `HasSeamProblem` yields these intervals:
  C++ `[0, 0.44758834428291155]` end **VERTEX** (104), `[.., 0.44823953478617956]` start VERTEX, end
  EDGE t=0.008715797478860355, `[.., 1]` mid FACE; Rust: the first crossing is **EDGE (103) at edge
  parameter 1.0** (the edge end) instead of the vertex. Everything else matches. Snapshot effect:
  `m_bBadNearMissSeam` C++ TRUE / Rust FALSE (`values.v[29]` 1 vs 0), `m_eBadNearMissSeamDir`
  (`v[30]`) C++ 0/1 vs Rust 4. Suspected cause: the boundary intersection of
  `CurveOnClassify` (vertex intersection or edge-hit-at-edge-end snapping in `query::face::p2`
  / curve classification), not query.face_p1. With the vertex found, the Rust side would reach
  `SmFace::FindVertexSector` (topology.face_p4) and defer, as C++ goes through it.
- **topology.brep_p3 (fixed here).** `brep_curve_solve` (OrientTrimmedSurfaces ray votes) solved
  against an empty `BrepCache` with no trees (`SM_ERR_NULL_POINTER` from
  `TopologyObjects::brep_trees`); it now builds them with `make_brep_cache`, as upstream's
  `CacheMakeOrValidate`. Only reachable once the face classifiers resolve (7 cases became
  failures in the combined run before this fix; deferred to topology.face_p2 after it).
- **Adapter hooks left out.** Giving `query::face::p3::LoopObjectCurves` and
  `topology::face::p2::UvLineObjects` `topology_entity` / `topology_handle` (patch kept outside
  the repo) resolves their edges as well; combined with the forwarding it shows 5 more
  topology.face_p2 failures in `CalculateAnInternalPoint` (internal_bss_tiny, extra00702,
  extra00744, extra00806, extra01154; earlier deferred at `SmEdge::GetCurve`). No face
  classifier runs in internal_bss_tiny, so these disagreements are not query.face_p1's.
  extra00744: C++ (0, 0, 5), Rust (-0.659, -0.2026, 4.952) on sphere.smb.
