# intersect.curve_class_p4 (`SmCurveClassification` part 4: the full constructor, `GetLoopCrossings`, `SmCurveInterval::GetIvlBound_EdgeuseObj`, `sm_SqueezeOutSmallestInterval`, 2026-10-16; WIP, no disagreements)

- **Topology through `ClassGeometry` hooks** (review of PR #137, the #150 pattern). The classification refers to
  objects through the handle `O: ClassObject`; a loop, face, face use, edge, edge use, vertex or vertex use
  handle reaches its B-rep through the defaulted hooks `ClassGeometry::topology_entity` (handle -> `(&Brep,
  EntityRef)`) and `topology_handle` (entity -> handle), CO-PORTED into `intersect::point_class`. Part 4 then
  calls the ported members: `topology::loopuse::loop_face`, `topology::face::p2::upward_faceuse`, the face's
  surface, `topology::edge::{edgeuses, curve, interval}`, `topology::edgeuse::p1::{edge, orientation}`,
  `topology::vertex::vertexuses_of_face`, `topology::vertexuse::{vertexuse_type, compute_uv_sector}` and
  `FaceProps::set_props_stage2` (`heal.face_props`) on the face's `(Brep, FaceId)`. A handle the adapter does
  not resolve is `InvalidInput` (a gap in the caller's adapter, not unported code). `topology::face::p2`'s
  `FaceClassifyObjects` and the difftest pool implement the hooks. `ComputeUVSector` takes `&mut Brep` in the
  port; with `bOkToMake` false it creates nothing, so part 4 runs it on a copy of the B-rep (an error it
  returns propagates instead of leaving partial outputs: the port's `compute_uv_sector` reports evaluation
  failures as errors). Only `SmEdgeuse::EvaluateBinormal` (`topology.edgeuse_p3`) remains a stub: part 4 calls
  the existing stub in `topology::edge` (now `pub(crate)`).
- **Other stubs.** `intersect.curve_class_p2` (PR #85, still open: `ReSet`, so the whole
  constructor; `ConnectIntervals`, so every squeeze; `GetMinUV` / `GetMaxUV`). `intersect.curve_class_p7`
  (PR #65, merged) is wired: `GetLoopCrossings` calls `CurveClassification::point_classifies_connected_to`
  (review round 2 of PR #137: 1076 -> 1318 passing, 2125 -> 1883 deferred, 0 failed).
- **UB-131 in `GetLoopCrossings`.** With face props seeded at stage `GAPSa` or `GAPS` and no face pointer
  (seed field 24 unset), `SetProps_Stage2` -> `SetProps_Gaps` returns early and `m_pFace->GetSurface()` runs
  on `NULL`. The Rust side no longer ignores the `InvalidInput` of `FaceProps::set_props_stage2`, and the FFI
  guard (`stage2_derefs_null_face`, simulating stage and face pointer as the `heal.face_props` validator
  does) rejects those scripts. A `NULL` loop at an index past 0 is still rejected by the shim: the handle
  type cannot represent `NULL` (upstream's `IsConnectedTo(NULL)` returns TRUE), so that input is not compared.
- **What the difftest compares** (one-face B-rep built by `SmBrepConstructor` in the shim, through the shared
  `harness/cpp/shim/sc_one_face_brep.h` that `heal.face_props` uses too, rebuilt record by record by
  `heal_face_props::rust_brep`; plane, cylinder and sphere faces (`SmSurfOfRevolution::CreateCanonical`, closed
  or open), vertex loops and edge loops of line edges with their NURB made on both sides (C++
  `SmPlane::DropCurve` of a UV trim curve reads it)): `GetLoopCrossings` over the real loops up to the touch
  list (the face of `loops[0]`, `SetProps_Stage2` through the hook or face props preset at stage `2a` or later
  by seeds, the props snapshot, the face's surface and zone tolerance, `IsClosed` / `IsDegenerate` of the
  classification curve and the return without crossings of a degenerate one) and over empty loop lists;
  `GetIvlBound_EdgeuseObj` on the real face for interval ends not on an edge (the face's upward face use and
  surface, then the `NULL` return). Deferred and so not compared: every touch list that reaches an unported
  `IsConnectedTo` (`topology.vertex` / `topology.edge`; reached after the seam sampling, which on a face-owned surface also defers on `SmFace::GetTolerance` in the
  surface's zone tolerance, `topology.face_p2`, a pre-existing stub of `surface.surface_p1` / `_p4`),
  `SetProps_Stage2` of a closed face (`SmFace::HasSeamProblem`, `topology.face_p4`), every edge end of
  `GetIvlBound_EdgeuseObj` (`SmEdge::GetCurve` in `PointClassification::curve_object`, a pre-existing
  `intersect.point_class` stub that the new hooks would let a later PR wire, then `GetMinUV`), the
  constructor and every squeeze of a non-empty classification. Not reachable with `SmBrepConstructor`: a face
  without surface (the `NULL` surface return of `GetIvlBound_EdgeuseObj`, and UB-124 in `GetLoopCrossings`).
- **Harness limits.** `GetIvlBound_EdgeuseObj` of an interval whose back pointer is another classification is
  refused (the port addresses intervals in their classification and has no back pointer). The C++ side runs
  the whole of `GetLoopCrossings`, including the parts Rust defers; the shim refuses (UB-124) loop crossings
  over loops of a classification without curve.
- **Wiring.** `topology::face::p2` (`classify_curve_on_face` with each upstream site's constructor
  arguments; `review_start_and_stop_on_seam_crv_classifies(geometry, ...)` takes a `ClassGeometry` for
  `ivl_bound_edgeuse_obj`; its `ClassHandle` gained loop, edge, face use, edge use and vertex use handles) and
  `intersect::topology_solver::p2` (`classified_curve_intervals`, with the `SmZoneTol3d` conversion of the
  solver tolerance; `CurveOnClassify` stays a `justified:` stub there: the solver's object adapter has no face
  -> `(Brep, FaceId)` / `CurveOnClassifyObjects` resolution, and the constructor defers on `ReSet` first) call
  the port, as do `intersect::curve_class::p6` (`sm_SqueezeOutSmallestInterval`) and
  `topology::proto_topology::p3` (the `SmCurveClassification(curve, interval, NULL, tol)` constructor).
  `topology::face::p2` also calls `query::face::p2::curve_on_classify` after the constructor; the
  `GetPointsInFace` site passes the upstream arguments (`dTol`, no borrowed data). `SetDoingCrossHatch` is not
  ported here (upstream sets it only in `SmFace::OutputPolygons`, outside this unit).
- **No `Kernel` under `Curve::knots`.** The constructor sites in `curve::crv_in_volume` (`compute_breaks`) and
  `curve::crv_on_surf` (`classify_against_surface`) have no `Kernel` in scope, and
  `CurveClassification::with_curve` needs one for its default tolerances; threading a kernel through the
  `Curve::knots` virtual is out of this unit's scope. Their stubs name what blocks them first,
  `UNPORTED(intersect.curve_class_p2): SmCurveClassification::ReSet` (then `intersect.curve_class_p8`); the
  porter of `intersect.curve_class_p2` must also bring a `Kernel` to those two sites.
- **Resolved: `Kernel` under `Curve::knots` (PR #437).** PR #437 threads `k: &Kernel`
  through the `GetKnots` / `GetNumberNaturalKnots` virtuals (`Curve::knots`, `Curve::number_natural_knots`
  and the CrvOnSurf, CrvInVolume, Composite, Offset and Projected overrides, plus `Curve::snap_to_knots`).
  The stack classification is `intersect::curve_class::p8::{uv_discontinuity_intervals,
  volume_discontinuity_intervals}`, one shared `FreeHandle` adapter (a free-standing curve against one
  surface or volume, temporaries included). `curve::crv_in_volume` calls it (all 2030 cases pass);
  `curve::crv_on_surf` does not yet, see the next item.
- **`GetKnots` writes its out-parameters before the classification can fail (PR #437).** Both overrides
  copy the base (UV) curve's knots, adjust the multiplicities to a cubic, and only then `SER` on the
  classification; their own callers (`GetNumberNaturalKnots`, `CalculateContinuities`, the base
  `SmCurve` members, `SmCurveCache::CreateBezierSegmentBlock`) ignore the status and read what was written.
  `Curve::knots_with_status` / `KnotsWithStatus` carry both; `knots_ignoring_status` is the read of a caller
  that ignores the status. A 2d base curve always fails `Intersect3DDiscontinuities` (`SM_ERR`), so every
  2d `SmCrvInVolume` reaches this path.
- **CrvOnSurf's `ReSet` stays a stub: the base `DropPoint` runs on a copy.** `SmCrvOnSurf::DropPoint` calls
  `this->SmCurve::DropPoint(...)` (three exits), and on some global-solve paths that reaches `GetKnots` on
  `this` (through the curve cache), which caches the breaks (`m_bNeedBreaks = FALSE`). The port calls
  `self.as_curve().drop_point_base(..)`: the base member runs on a `Curve` copy, and `Curve`'s `&self`
  members compute an override's knots without storing them (`knots_ref`). With the classification wired, the
  dropped point matches but 17 generated `curve.crv_on_surf` drop cases end with a different break cache
  (`need_breaks` and the breaks in the state record). Caching the breaks after every base drop is wrong
  too (41 cases then differ: C++ caches only on some paths). Fix: let the base members keep an override's
  caches (e.g. run them on the `CrvOnSurf` itself, or give its caches interior mutability), then wire
  `classify_against_surface` to `uv_discontinuity_intervals`.
- **Upstream defect** UB-124 (`plan/upstream-issues.md`): the face, surface and curve are dereferenced
  before the functions' own `NULL` checks.
