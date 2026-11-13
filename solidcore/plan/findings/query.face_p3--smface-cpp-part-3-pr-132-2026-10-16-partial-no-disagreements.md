# query.face_p3 (`SmFace.cpp` part 3, PR #132, 2026-10-16; partial, no disagreements)

- **Builds on PR #129** (`query.face_p4`, which merged #122 `heal.face_props` and #85
  `intersect.curve_class_p2`): the bisector needs `SmFaceProps`, `SmClassifyLoopIO` and
  `SmCurveClassification::ReSet`. Merge those first.
- **What is compared.** `CalculateTightBoundingBox` of SmPlane faces (the box of the
  vertices and the edges' tight boxes) and the early return of `SectorClassifyCurveOn` at
  a singularity whose edge use's radial is the second edge use: 212 cases compare values.
  Everything past those defers on units not ported yet: the global solves and
  `DropPoint` of a face-owned surface need `SmFace::GetTolerance` (the port's surface
  cannot reach its face; tagged `topology.face_p2`), the sector classification
  `SmEdgeuse::EvaluateBinormal` (`topology.edgeuse_p3`, PR #117), and
  `SmFace::CurveOnClassify` the `ReSet` stub inside it (`intersect.curve_class_p2`).
- **Face-owned surface boxes.** `IsPlanar` / `CalculateBoundingBox` of a face's B-spline,
  plane or surface of revolution expand by the face tolerance; the port's `Surface`
  methods defer on that (`UNPORTED(topology.face_p2)`). `calculate_tight_bounding_box`
  computes the unexpanded boxes and expands them by the face's tolerance itself (the
  expansion is the last step upstream). The general fix (letting a face-owned surface
  reach its face tolerance) would undefer about 250 cases here and many elsewhere.
- **Handles for new curves.** The UV line of `sm_FaceClassifyUVLine` and the curve on
  surface and UV trim line of the bisector live in the caller's `ClassifyCurveObjects`.
  `LoopObject` gained `from_face`, `from_face_surface` and `from_classify_curve` (default
  `None`); `query::face::p4::classify_loops` now returns `InvalidInput`
  (`NO_CURVE_HANDLES`) for a handle type that cannot name them (`EntityRef`) instead of
  ignoring the bisector's failure as a status (an ignored failure would skip the
  bisector unnoticed). The `query.face_p4` and `heal.face_props` runners now use handle
  enums that can name them; the 11 `heal.face_props` cases that deferred on the bisector
  (vertex loops, for which it returns at once, as upstream) now pass, so that unit is
  green.
- **Harness.** `sm_FaceClassifyUVLine` is file-static: its cases run
  `CalculateAnInternalPoint`. `sms_FindAndClassifyLoopBisector` is not declared in a
  header; the shim declares it. The shim `#include`s the `query.face_p4` shim (entry
  point renamed) for the B-rep builders, so both units build identical B-reps.
- **Main merge, 2026-10-18.** `SmEdgeuse::EvaluateBinormal` (PR #117) is wired: the 106
  sector deferrals now pass or defer further in (`SmSurface::DropCurve`,
  `SmFace::CreateUVTrimCurves`). Main's `CurveOnClassifyObjects` now names every boundary
  object (vertex, edge, uses, edge and trim curves) and resolves them through
  `ClassGeometry`: `LoopObject` gained `from_topology` / `as_topology`, `from_edge_curve`
  / `as_edge_curve` and `from_trim_curve` / `as_trim_curve` (default `None`; `as_loop` and
  `as_vertex` became default `None` too), and `LoopObjectCurves` borrows the B-rep to
  resolve them. `PointClassify` and `SmEdge::IsSeam` now take the B-rep mutably; the tight
  box and the bisector run them on a private copy. The `query.face_p4` shim's export
  appends the shared shell state (as main's `query.face_p2` shim does) for
  `topology_face_p2::import`. The `heal.face_props` notes' "green" from the first round
  was overtaken by main's richer runner (closed surfaces of revolution defer at
  `HasSeamProblem`); it stays partial. 408 pass, 393 deferred, 0 failed.
- **Review fixes, 2026-10-19.** (1) The library's healer handle
  `topology::brep::p1::HealLoopHandle` was a bare loop id, so `SmBrep::HealBrep` reaching
  the bisector through `SetProps_Stage3` got `InvalidInput(NO_CURVE_HANDLES)`, which
  `FaceProps` returns as a status and `heal_brep` swallows: the healer went on with a
  result upstream would not produce (before #132 that path deferred). No runner reaches it
  (the heal_data_p1 Api runner heals an empty B-rep). It is now an enum naming the face,
  surface, topology, bisector curves, edge curves and trim curves. (2) Upstream's bisector
  writes `rbClassifyCrv_StartOnSeam` / `StopOnSeam` (references to `ClassifyLoops`'
  variables) before failing on e.g. `SER(DropPoint)`, and `ClassifyLoops` ignores the
  status and reads them; the port kept the previous target loop's flags on a failure.
  `find_and_classify_loop_bisector_into` now writes them as upstream does. Not visible in
  difftest until `GetLoopCrossings` (`intersect.curve_class_p4`) lands. (3) Coverage, said
  plainly: values are compared only for `CalculateTightBoundingBox` (192 cases) and
  `SectorClassifyCurveOn` (96). `sm_FaceClassifyUVLine` (op internal) and
  `sms_FindAndClassifyLoopBisector` (op bisector) compare none: every case defers
  (`SmFace::GetTolerance` in DropPoint, `topology.face_p2`; DropCurve,
  `intersect.surface_drop_lib_p3`) or both sides return the same status. (4) The stale
  `SmCurveClassification::ReSet` stubs in `query::face::p2` and `heal::face_props` were
  wired here and, identically, by PR #85 on main (merged into this branch); main's
  `query.face_p2` runner fix (the shim's `CreateLineSegment` line and a classification
  with one interval) is the one kept. With them, CalculateAnInternalPoint cases defer on
  `intersect.surface_drop_lib_p3` instead of `intersect.curve_class_p2`.
  (5) `--fuzz 300` hit a C++ timeout (a tight box over a B-spline face with a `-1e300`
  control point ran over 10 minutes; `1e10`..`1e200` take 20-55 s). The FFI guard rejects
  tight-op B-rep values beyond `1e6`; generated and fixture cases stay below `1e4`;
  `--fuzz 300` now passes (0 crashes, panics or timeouts).
  (6) Known gaps left: the sector shim always passes eu1 `SM_OT_SAME` / eu2
  `SM_OT_OPPOSITE`, so the `eEU1Orientation == SM_OT_OPPOSITE` tangent flip and PART 2's
  eu2/mate vertex use choices are never compared; the tight op's `has_trim_cache`
  output is not asserted (5 compared cases had a C++ `TRIMSRF` cache where the Rust
  passes no trim cache to PointClassify, and still agreed); the tight box drops partial
  `GlobalPointSolve` solutions on failure (upstream appends `sSols_1` even when
  `GlobalPointSolve_1` fails; `Surface::global_point_solve_into` has the same gap in
  `surface.surface_p4`), not seen in any case. The 398 review adversarial cases are
  `fixtures/query.face_p3/review_adversarial.json` (none is undefined in C++).
