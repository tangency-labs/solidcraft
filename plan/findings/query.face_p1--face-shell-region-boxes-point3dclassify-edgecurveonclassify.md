# query.face_p1 (face/shell/region boxes, `Point3DClassify`, `EdgeCurveOnClassify`, `SmApiEvaluateCurve`; takeover of #105, 2026-10-18; partial, no disagreements)

- **Face-owned surfaces in imported B-reps.** Upstream face surfaces are owned by their
  face, and the point solvers read that: `LocalPointSolve_1` takes its loose tolerance
  from `SmTol::GetZoneTol3d(surface)` (the face tolerance) and `GlobalPointSolve` tests
  trim containment through the face unless the surface cache's point test is off. The
  `topology.edgeuse_p1` import leaves arena surfaces unowned, so the Rust solvers took
  the standalone path (loose tolerance `0.01` for intersections, no trim test) and
  disagreed with the C++ on near-boundary and out-of-tolerance points (5 of ~480
  generated `Point3DClassify` cases; the others agreed by coincidence). The
  `query.face_p1` runner now owns each imported face surface by its face, so every
  `Point3DClassify` case reaches the existing face-owner stubs
  (`surface::surface::p1::zone_tol_3d`, `UNPORTED(topology.face_p2)`
  `SmFace::GetTolerance`; `surface::surface::p4` trim containment,
  `UNPORTED(query.face_p2)` `SmFace::PointClassify`) and is deferred. Those stubs can't be
  wired while a `Surface` cannot reach its face's `Brep`. Other runners that import
  through `topology.edgeuse_p1` and run point solves on face surfaces
  (`topology.vertexuse`, `intersect.curve_class_p1` callers) have the same latent gap.
- **Face boxes** expand a B-spline/plane surface box by the face's own tolerance (what
  `SmBSplineSurface::CalculateBoundingBox` reads through `GetFace()`), computed unexpanded
  and expanded in `query::face::p1`, so they don't depend on the owner link.
- **`m_pFU` is not in the edge-use export.** `SmFace::GetVertices` / `GetEdges` (co-ports
  in `query::face::p2` / `topology_traverser`) and `EdgeCurveOnClassify` start from the
  face's primary face use; the runner patches it in from the shim's `FaceFu` op (as
  `topology.loopuse` does). Runners that call face boxes on that import without the patch
  get empty vertex/edge lists for linear surfaces.
- **`SM_SUCCESS` is 1000.** `sc_query_face_p1_curve` reports the raw `SmApiStatus`; the
  runner maps 1000 to 0. The WIP fixtures of #105 compared 1000 against 0 and never
  passed natively.
- **macOS / Apple clang** (local runs only; CI is Linux GCC): `curve_projected_curve.cpp`
  (explicit instantiations after the anonymous namespace closes) and
  `geom_geom_utility_p1_static.cpp` (`using ::SmMatrix;` before the header's
  `class SmMatrix;`) don't compile, and the FFI build script links `-lstdc++`. Worked
  around locally; not changed in this PR.
- **`ClassifyProblem`** is defined here, in `query.face_p3`'s open PR #132 and in
  `topology.face_p4`'s open PR #239, with the same variants and discriminants; whichever
  lands later should keep one copy and re-export it.
- **Generic-handle callers.** `boolean::topology_intersector::p1` (face box,
  `Point3DClassify`), `intersect::curve_class::p7` (`EdgeCurveOnClassify`,
  `Point3DClassify`), `intersect::topology_solver::p1` (`Point3DClassify`) and
  `geom::object` (face box) hold opaque face handles without the `Brep` that
  `query::face::p1` takes, so they stay unwired and are marked `DISPATCH GAP`, not
  `UNPORTED(query.face_p1)`; their deferral label is still `query.face_p1`. Three more
  `Point3DClassify` callers landed with #259 and are marked the same way:
  `boolean::topology_intersector::p2` (opaque handle), and `topology::brep::p9` /
  `topology::face::p3`, which hold `&Brep` where `point_3d_classify` takes `&mut Brep`
  (for `query::face::p2::point_classify`); wiring those two means lending the B-rep
  mutably down their call chains. Their cases would still defer inside it today.
