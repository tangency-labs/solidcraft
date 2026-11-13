# topology.brep_p2 (PR #149): coverage limits of the difftest

- **Normalised geometry.** The shim (`harness/cpp/shim/topology_brep_p2.cpp`) replaces every
  analytic face surface other than `SmPlane` and every edge curve other than `SmLine` by a plain
  `SmBSplineCurve` / `SmBSplineSurface` copy of its NURB before running the op (as the
  `heal.heal_data_p2` shim does), because the `topology.vertexuse` importer only rebuilds
  B-splines, planes and lines exactly. So the evaluations behind `LocalCurveSector`,
  `GetMinEdgeBBoxSize`, `MateLaminaEdgeToSurface` and `RayFire` never see analytic cylinders,
  cones, spheres, tori, surfaces of revolution/extrusion or conics. None of the unit's own code
  branches on those kinds (the one kind test, the `SmPlane` mating surface extension of
  `MateLaminaEdgeToSurface`, is built as a real `SmPlane`), but the geometry calls they reach are
  only compared on B-splines here.
- **Ops without compared values.** `RayFire` defers every case (`SmBrep::CalculateBoundingBox`:
  `query.brep`) and `ReplaceSurface` every case (16: `SmFace::SetSurface`'s `SmFace::Notify`,
  one of the stubs `topology.face_p3` left in `topology/face/p1.rs`).
  `MergeCurvesOnSurface` compares its early statuses (short or no curves); past those the
  wired curve classification constructor (`intersect.curve_class_p4`) reaches
  `SmCurveClassification::ReSet` (`intersect.curve_class_p2`, 1 case).
- **`MateLaminaEdgeToSurface` past its early returns (review 5578349117 round).** With
  `topology.face_p3`'s `SetTolerance` / `Notify` / `CreateUVTrimCurves` wired in `p2.rs`, 12
  more mate cases pass (2 compared values each); 20 defer on
  `SmPlane::CreateIsoParametricCurve` (`surface.surface_cache_p1`) and 8 on
  `SmSurface::DropCurve`. The UV trim curve `CreateUVTrimCurves` builds on a plane is an
  `SmLine`, which upstream is an `SmBSplineCurve` (`GetOrCreateGwNurbPointer` makes its NURB
  through `MakeNurb`); `uv_trim_curve_copy` accepts a `CurveKind::Line` the same way instead
  of refusing it (23 cases failed before).
- **Canonical numbering in the shared graph export (`boolean_merge_p1::graph`).** The C++ shim
  numbers records in first-seen order of its walk (regions from `GetRegions`, then every edge
  from `GetEdges`; per edge use: edge, owner, cw, ccw, vertex use, next, last), while the Rust
  arenas append new records, so after an edit that creates or deletes topology the raw arena
  indices differ from the C++ numbers on the same graph. `Canon::of` replays the shim's walk on
  the Rust B-rep and both the graph and the id slots the `topology.brep_p9` and
  `boolean.merge_p1` runners record are mapped through it (on a C++ import it is the identity).
  This fixed 97 `brep_p9` and 3 `merge_p1` mismatches once `SmBrep::Notify` stopped deferring
  them.
- **`LiftCurve` keeps the iso curve's kind.** `sm_ExtractIsoCurve` returns the iso curve as
  the surface's own kind made it (an `SmLine` on a plane, an `SmCircle` on a surface of
  revolution), and `LiftCurve`'s callers (`ExtendTrimEdges` stores it as the offset edge's
  curve) dispatch on it; `LiftedCurve::curve` is now a `Curve` rather than a B-spline (6
  `offset.offset_geometry_creation` mismatches). On a face the offset runner imports as its
  B-spline part the Rust runs the B-spline `CreateIsoParametricCurve` where the C++ runs the
  subclass's override (an `SmCircle` on a surface of revolution), so `ExtendTrimEdges` and
  `EdgeuseOffset`'s `ExtendTrimEdges` path defer (`harness:analytic-geometry-export`) when
  the lift's UV curve (the edge use's or its radial's UV trim curve) is a degree-1 iso line
  and the offset edge use's face is not exact. 6 cases that passed on main are in that class
  and now defer (the override returned a B-spline there, so they agreed by coincidence:
  gen0009, gen0119, gen0148, gen0376, gen0520, gen0707); gen0683 (an `EdgeuseOffset`, circle
  14021 against B-spline 14033) was the mismatch that showed the class.
- **Exact `SmSurfOfRevolution` faces in the offset runner (#149 review round).** The shim's
  `Export` now appends, per face, the binary `WriteToDB` record (version 33) of a surface that
  is exactly an `SmSurfOfRevolution`; the runner reads it back with
  `SurfOfRevolution::read_from_db` (the face keeps its owner) and marks the face exact, so
  `sm_ExtractIsoCurve` runs the revolution's own `CreateIsoParametricCurve` on both sides. The
  runner's `put_curve` exports every `SmBSplineCurve` kind as the fixture's does (type and
  NURB; the lifted circle is an `SmCircle`). The non-local `FaceuseOffset` stop (HANG-26) is
  now keyed on the face's exported type (any subclass other than `SmPlane`), as the shim's,
  not on exactness. Of the 7 lift cases above, the 4 on surfaces of revolution (gen0009,
  gen0148, gen0520, gen0683) pass again; gen0119 and gen0707 (`SmSurfOfExtrusion`) and gen0376
  (`SmSphere`) stay deferred: the Rust `SurfaceKind` has no extrusion or sphere variant to
  import them as (issue #76). Overall 861 cases, 552 pass (was 542), 0 fail, 309 deferred: 10
  `harness:analytic-geometry-export` deferrals pass (105-408 values: the 4 lifts plus
  gen0070, gen0178, gen0187, gen0597, gen0698, gen0767, whose B-reps' only inexact faces were
  revolutions) and 9 now defer on `intersect.surface_drop_curve`; 107 harness deferrals remain
  (extrusion, cone, sphere, torus faces).
- **Make ops (after `topology.shell`, #154).** `MakeShellVertexTopology`,
  `MakeEdgeVertexTopology` and `MakeEdgeTopology` run (76 cases flipped from the shell stubs)
  and output, after the status, the B-rep's topology read back through fresh list queries on
  both sides (per region its shells as kind and vertex / first wire edge, per vertex its
  vertex-use count, per edge the vertices and shells of its primary edge use and that use's
  mate): 17 to 86 values per case. A status alone would have been a vacuous pass (the makers
  return `SM_SUCCESS` on every reachable input). `IsSheet` / `IsOrientedSheet` run through #145's
  `GetShells` (66 cases, status and answer).
- **Passes without numeric content.** Of the 1290 passes, 262 compare no computed value: the 162
  `Notify` passes compare the constant `0` the shim writes (no B-rep cache on either side, see
  below), the 91 `MateLaminaEdgeToSurface` passes the early `SM_ERR`, the 3
  `MergeCurvesOnSurface` passes a status (the generated curves are 1e-7-scale against
  `tol=1e-3`, so only the "too short" skip runs), `SwapUV` 2 passes on faceless B-reps and
  `RebuildUVTrimCurves` 4 on B-reps without UV curves. The queries, `SetTolerance`,
  `GetMinEdgeBBoxSize`, `LocalCurveSector` and `DeleteStaleCaches` compare real values.
- **`DeleteStaleCaches`.** The shim first deletes the `SM_OC_TRIMSRF` caches a file read leaves
  (e.g. on `Endcap.smb`'s face 0), then creates one per face with `m_bFaceWasModified` set from
  the case's `modified` mask (only for faces in the `create` mask), runs the op and outputs per
  face whether a cache survives. The Rust side seeds a caller-held
  `BTreeMap<FaceId, TrimSrfCache>` the same way, so the deletion branch is compared.
- **Remaining `topology.face_p2` deferrals.** The 12 are `SmFace::GetTolerance` stubs in
  `intersect/point_class.rs` (main's generic `ClassGeometry` code, which has no B-rep access);
  `topology.face_p2` is ported but those call sites cannot reach it until `ClassGeometry` carries
  the B-rep. Not this unit's stubs; left as they are.
- **FFI restrictions.** `MateLaminaEdgeToSurface` rejects a negative extension factor
  (`HANG-18` in `plan/upstream-issues.md`: the C++ never returns; the Rust returns
  `InvalidInput` there too), and a `Mate` whose extension distance `dMaxDist * factor`
  is not finite (`HANG-9`: a mating surface so far from the edge that the squared point
  distance overflows, found by `--fuzz`; the FFI first runs a `MateProbe` op that replays
  Mate's early returns and then computes upstream's `dMaxDist` with the same calls, so an
  early `SM_ERR` is still compared; the Rust stops there with `NotConverged`), and a
  degree >= 2 B-spline face under `init_gaps` with a corner control point coordinate past
  `1e154` (`HANG-28`: the shared fixture's `GetVertexFaceGap` slows down from `1e155` and stops
  returning around `1e156` on v-degree-1 faces, measured on x, y and z; the first bound,
  `2e156`, was inside that slow band; the exact condition is not pinned down, so shapes that
  return, e.g. v-degree 2, are refused too; a rational corner is bounded at `1e151` on both
  its stored (homogeneous) and its Cartesian coordinate, since a small weight moves the vertex
  out and a large one the stored value, measured with corner weights `1e-3` to `1e3` and the
  other weights `1e-3` / `1e3`). `LocalCurveSector` requires `t0 != t1` (finite; extrapolated
  intervals are allowed, and an inverted pair is swapped by `SmExtent1d(t0, t1)` on both
  sides) and an integer point class 100..=110; the object kind must match only
  for face/edge/vertex classes (102/103/104), every other class takes upstream's default
  `SM_ERR` arm with a null or vertex object. The orientation is an `SmOrientType` code 0..=2
  decoded exactly (lesson 22): `SM_OT_UNKNOWN` is neither `SAME` nor `OPPOSITE` upstream
  (`LocalCurveSector` and the start parameter of `RadialSectorOfCurveClassify` test
  `== SM_OT_SAME`, its tangent flip at `SmEdge.cpp:2110` tests `== SM_OT_OPPOSITE`); the runner
  first decoded every code other than 1 as `Opposite`. With the exact decode the 52
  orientation-0 fixtures give 19 passes comparing status and region and 33 deferrals, as
  before (unit counts unchanged). For `topology.edgeuse_p3`: 32 of those deferrals
  (`SmEdgeuse::EvaluateBinormal`, edge/vertex classes) reach the one site where
  `SM_OT_UNKNOWN` acts as `SAME` (the tangent flip, `SmEdge.cpp:2110` =
  `query/edge.rs:953`); re-check them when they flip to compared. Re-checked after #117
  merged: of the 52 orientation-0 fixtures 46 pass (27 of those 32 among them), 4 defer on
  `SmSurface::DropCurve` (`intersect.surface_drop_lib_p3`) and 2 on `SmFace::GetTolerance`.
- **`heal.heal_data_p1` box cases.** Wiring `SetTolerance` let `SmApiHealBrep` run past
  `Fix_TolSizes` on the heal_data_p1 runner's stand-in: an empty Rust `Brep` against the C++
  `SmApiCreateBox` box (7 cases, C++ status 1000 vs Rust success). The runner now defers those
  cases to `primitive.api_primitives` (`SmApiCreateBox`) instead of comparing different inputs.
- **List order.** The port's `GetFaces` / `GetEdges` / `GetVertices` order is the B-rep's list
  (`topology.brep_p1`); the shim registers those lists before its walk so the import order is
  upstream's list order (without that, `GetEdgeuses` disagreed on `Mouse.smb` and
  `box_with_large_fillet.smb`).
- **B-rep cache.** `SmBrep::Notify`'s spatial-tree updates work on a caller-held
  `SM_OC_BREP` cache (`brep::p2::BrepCacheSlot`). `topology::edge::notify` / `set_curve` pass
  none (adding the slot to `edge::NotifyEnv` would break the struct literals of other open PRs,
  and no Rust caller can build a B-rep cache yet: `make_brep_cache` needs `query.brep`'s
  `CalculateBoundingBox`). The difftest compares `Notify` on fresh B-reps, where neither side has
  a cache; the tree add/remove paths are not compared. Thread the slot through `NotifyEnv` once
  `query.brep` lands.
- **File statics.** `sm_GetPointFromSol` and `sm_CheckAndFixUVTrimCurves` cannot be called from a
  shim (static in `SmBrep.cpp`); they are covered by Rust unit tests only.
- **Back-edges into this unit after the #145 / main merge.** `topology.brep_p6`'s
  `TestClosureSplitRegion` (#169) now calls `brep::p2::notify` with `PRE_EDIT` and
  `SPLIT_IN_BREP(old, old, new)` (statuses ignored, `SmBrep.cpp:27954-5`); its split API takes the
  caller's `NotifyEnv` (overseer ruling: thread the env, the landing unit's job). The new region
  gets its default `SmTopology` record when the split makes it (as `new SmRegion`), because
  `SPLIT_IN_BREP` propagates the old region's record to it. The 6 brep_p6 cases that deferred on
  `SmBrep::Notify` only relabel to the next call, `CalculateBoundingBox` (`query.brep`). The
  `ShellRayWorld::shell_ray_sector` default of `topology.shell` is retagged
  `UNPORTED(query.brep)`: the generic default has neither the B-rep nor an entity
  classification, and the world over the B-rep cache (`query.brep`) overrides it with
  `brep::p2::local_curve_sector`.
- **`--fuzz 2000`: FUZZ-PASS with 2 differs (rv-brep_p2's pre-review of `3a6ea1df`,
  2026-10-20).** Both are `MateLaminaEdgeToSurface` with the mating plane's origin pushed
  to about +/-9.2e18. `values.out[1]` is Mate's status: C++ 1001 (`SM_ERR`), Rust 1006
  (`SM_ERR_NULL_POINTER`). The inputs:
  - `fuzz00488`: base `mate_ellips_ts2_e0`, mutation
    `/surface/plane/o/1 = 9223372036854775807`;
  - `fuzz00815`: base `mate_surf_rev_e1`, mutations `/surface/plane/o/1 =
    -9223372036854775808` and `uv_ext = -1e300`.
  The Rust stops at the 6th of Mate's 21 distance samples. The extended mating plane's
  `Surface::global_point_solve` (`surface.surface_p4`'s base `GlobalPointSolve` with the
  plane's fast drop, `surface.plane_p1`) returns an empty solution array, and Mate raises
  `null_deref("point solve without solution")`, where upstream reads `sSolutions[0]`
  (`SmBrep.cpp:22222`). The C++ fails the same solve with `SM_ERR`, which Mate's `SER`
  propagates. So the disagreement is in the callee (on the Rust side the in-domain test
  drops the fast solution of a plane 9.2e18 away; the C++ errors), not in this unit's
  logic, and the generator never makes such origins. difftest counts `differs` as a note,
  not a failure. Owners of `surface.surface_p4` / `surface.plane_p1`: both inputs should
  become fixtures once the solve's status agrees.
- **Notification keys (rv-brep_p2's S1).** `region_only_key` keys a region by its arena
  identity (`loopuse::notify_ref`) instead of a bare index, so the heal path's tracker
  (`entity_of_notify_key`, the inverse of `notify_ref`) decodes a `SPLIT_IN_BREP` region of
  `p6::split_closed` as itself. Before, region `r >= 1` decoded silently as `Brep(r - 1)`.
  It was latent: the op defers at `CalculateBoundingBox` first. Every other entity keeps
  the record-less `(0, u64::MAX)` key, which the decoder rejects. Unit test
  `region_only_keys_decode_as_the_tracker_reads_them`.
