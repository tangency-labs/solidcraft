# surface: owning face's zone tolerance cached on `Surface` (branch `surface/owner-zone-tol`, 2026-10-20)

Upstream reads `GetFace()->GetTolerance()` from a surface in ten places (`SmTol::GetZoneTol3d(SmObject*)`
SmTol.cpp:775, `SmBSplineSurface::CalculateBoundingBox` SmBSplineSurface.cpp:797-806, `SmSurface::DropPoint`
SmSurface.cpp:10432, `LocalSurfaceSurfaceIntersectXYZ` SmSurface.cpp:29051, `ClassifyCurveSurfaceXsectPoint`
SmSurface.cpp:13373, `SmPlane` zone tolerance, `IsPlanar`, approx tol, point_class, surface_cache_p2). The port's
surface owner is an opaque handle, so every site returned `Unported(topology.face_p2)`. `Surface` now caches the
owning face record's `tolerance`, keyed by the reached face handle (a different or cleared owner never reads it);
`Brep::sync_face_surface_tolerance` keeps it current at every place a face record's surface or tolerance changes.
A face owner without cache still returns `Unported`. `surface.surface_cache_p1` `mark_face_containment` (an opaque
node face handle) is not covered. On main 862746f4 (`--green`, all units): `topology.face_p2` deferral label 3495 ->
766, `surface.surface_cache_p2` 223 -> 0, total deferred 24839 -> 22724, +2039 pass over 15 units, +76 FAIL in
`query.face_p3` (75) and `query.face_p1` (1), both below (exposed, not caused: each case read the face tolerance
correctly; the next divergent step is elsewhere). After both fixes below: 0 FAIL, +1751 pass over 15 units, total
deferred 24839 -> 23088 (`query.face_p3` 75 and `query.face_p1` 289 now defer on unported callees).

### topology.trimming_tools: stale `TT_NOT_RUN` shim gate

`harness/cpp/shim/topology_trimming_tools.cpp` returned `-3` (`TT_NOT_RUN`) for `b_check_face`, `b_orient` and
`b_order` on every face with loops, written while `SmFace::ComputeLoopOrientation` was unported. `topology.face_p3`
ported it; the Rust side kept deferring only through `SmFace::GetTolerance`. With the cache the Rust side
completes, so 305 cases (104 check_face, 102 orient, 99 order) compared a real result against `-3`. The shim now
runs `CheckFace` / `FixTrimLoopOrientation` / `FixTrimLoopOrder` in full, guarded with `-2` (mirrored in the
runner) where a loop lacks its loop use, a face with loops lacks surface or UV trim curves, or `CheckLoop` would
dereference a null UV extreme (17 cases). The fixtures never reorient a loop (orient always 0, order always
`false`): those two ops are compared on the no-change path only. Follow-up: misordered / misoriented loop fixtures.

### topology.brep_p1 `edge_edge_blend` on analytic face surfaces: harness input mismatch, not a port bug

Inputs: `surf_rev.smb` (`crates/difftest/fixtures/topology.brep_p1/upstream_files.json#1518` flag=1 eu1=0 eu2=1
dir=0, `#1520` flag=3 eu1=1 eu2=2 dir=1). C++ `out[979]` = 3.4935082719274374e-1 vs Rust 3.4935082670870377e-1
(1.4e-9), growing to 7.3e-4 (`3_1_2_1`: 5.3e-4). Previously deferred on `SmFace::GetTolerance`. Cause: the
shared `topology.edgeuse_p1` export hands Rust the faces' `SmSurfOfRevolution` as its B-spline part. In
`GetSampleValues` -> `DropPoint`, C++ succeeds in `SmSurfOfRevolution::DropPointFast` (SmSurfOfRevolution.cpp:2356),
while Rust's B-spline `drop_point_fast` is the base `SM_ERR` and it uses `LocalPointSolve` (bit-identical to C++
`LocalPointSolve`). First drop difference: u = 0x3fcdfd6b2cbee1af vs 0x3fcdfd6b2cbee37e (5.5e-14), amplified by
the G2 blend fit. With the C++ faces flattened to the same B-spline part (temporary shim switch), all 159 compared
blend cases match at 0. surf_extrude (`SmSurfOfExtrusion::DropPointFast`, up to 9.4e-10) and surf_rev 2_0_2_0
(8.9e-11) show the same drift below 1e-9. Resolution: the runner defers blends (flag != 0) whose faces are
non-exact surfaces (`harness:analytic-geometry-export`, +27 cases: 2 FAIL, 25 pass). Exporting revolution /
extrusion surfaces exactly in `sc_edgeuse_graph.h` would let them be compared.

### query.face_p3 `bisector`: `LoopObjectCurves` has no `vertex_face_gap` (75 FAIL, previously deferred on `SmFace::GetTolerance`)

`sms_FindAndClassifyLoopBisector` calls `SmFace::CurveOnClassify(TRUE, cc, TRUE /* bHasBigGaps */, TRUE, ..)`
(SmFace.cpp:2884-2892). With big gaps and a face, `IntersectVertices` reads
`pVertex->GetVertexuseOfFace(pFace)->GetVertexFaceGap()` (SmCurveClass.cpp:14716-14744). The port's
`vertex_big_gap_tolerance` (`crates/solidcore/src/intersect/curve_class/p5.rs:163`) asks
`ClassGeometry::vertex_face_gap`, which `query::face::p3::LoopObjectCurves` (`crates/solidcore/src/query/face/p3.rs:549`)
does not implement (trait default `None`), so every call returns `InvalidInput("unresolved classification
vertex/face gap")`. The bisector ignores that status as upstream does (`ignored(curve_on_classify(..))`), so the Rust
side records no intervals: e.g. `bisector_Mouse_f2_t0` `values.interval_bounds.len` C++ 6 vs Rust 2,
`bisector_holed_box_top_flat` `ints.interval_classes[0]` C++ 103 vs Rust 100. A temporary probe showed this error on
all 75 classification calls of the failing cases and on no other path. Fix: implement `vertex_face_gap` on
`LoopObjectCurves` (vertex use of the face, `has_vertex_face_gap`, cached or computed `SmVertexuse::GetVertexFaceGap`
length, 0 when absent); it needs a kernel and a non-caching gap computation, since the adapter holds `&Brep`.
More generally, an adapter gap reported as `InvalidInput` is indistinguishable from an upstream status under
`ignored`: no `ClassGeometry` implementation provides `vertex_face_gap` today.

Resolved: the gap is a B-rep property, so `query::face::p2::curve_on_classify` (which has the B-rep and kernel)
now computes it for every caller, not only `LoopObjectCurves`: with `has_big_gaps` it reads
`vertex::vertexuse_of_face` -> `vertexuse::vertex_face_gap(.., false)` length (0 without a gap) of each face
vertex on one private B-rep snapshot per call (it keeps the gap caches across both passes, as upstream's
`GetVertexFaceGap` caches into the B-rep), and answers `ClassGeometry::vertex_face_gap` for those vertex handles
through a wrapper around the caller's geometry. With the gap tolerance in place, all 75 cases reach
`SmCrvGeometry::GlobalPointSolve` on the bisector's `SmCrvOnSurf`, whose `Curve::knots` stops at the known
`UNPORTED(intersect.curve_class_p2): SmCurveClassification::ReSet` (`crates/solidcore/src/curve/crv_on_surf.rs:218`,
no `Kernel` under `Curve::knots`): 75 FAIL -> 75 deferred. `query.face_p3`: 543 pass / 0 fail / 656 deferred.

### query.face_p1 `gen0160` (1 FAIL, previously deferred on `SmFace::GetTolerance`)

Generated `Point3DClassify` on a 1e-3 x 3e-3 bilinear-in-u B-spline sliver whose u=0 edge collapses onto the z axis
(`generated:seed=4634842130399439208`), point offset ~5e-4, `src_tol` 0.01, `pc_tol` 1e-8, face tolerance 1e-5.
`values.pc[0]` C++ 102 (FACE, uv 0.720517876766358, 0.823164791473039) vs Rust 104 (VERTEX, no uv). The global point
solve's UV is bit-identical on both sides (probe), and the three face-tolerance reads (`is_on_boundary`,
`local_point_solve_1`) all return 1e-5 as in C++; the divergence is inside `query::face::p2::point_classify`, which
reads no surface face tolerance. Not localised further.

Resolved (C++ probe, hunks in the worktree's `GEN0160-PROBES.diff`, reverted): after `Point3DClassify` the face
surface has an `SmTrimSrfCache` with `m_bHaveTSurfaceCache` 1, `m_bFaceContainmentDone` 1, `m_bFaceWasModified` 0,
`GetSurface()->GetFace() == this`; the one leaf node at the UV is `SM_NC_INSIDE`. The face's 4 vertices (tolerance
1e-5) are 1.083e-3 from the surface point, inside the vertex test's `max(1e-5, 1e-5, GetTolerance()=src 0.01)`.
C++ `PointClassify` at the same UV with the cache dropped returns VERTEX (104), as Rust. Cause: upstream
`Point3DClassify` calls `smsurf_GetSurfaceCache(pSurface)` first (SmFace.cpp:12792), which builds the trimmed
surface cache with its face containment; `PointClassify` then classifies through the inside leaf and returns FACE
before the vertex test. The port skipped that call (its header claimed a fresh cache is never
face-containment-complete, which is wrong for a face with loops: SmTrimSrfCache.cpp:330-347). Fix:
`point_3d_classify` now builds it (`make_trim_srf_cache`, else `make_surface_cache`) and passes it to
`point_classify`. The trimmed cache's face walk is unported (`Unported("SmFace handle of a surface's face",
topology.face_p2)`), so every `Point3DClassify` case on a face-owned surface now defers there: the 288 point cases
that passed with the skipped call (the cache path and the vertex/loop path agree outside such tolerance edges)
plus `gen0160`. `query.face_p1`: 1580 pass / 0 fail / 644 deferred (main 862746f4: 1570 / 0 / 654). They become
comparable with `surface.surface_cache_p1` `mark_face_containment` and the `TrimSrfCache` face walk.
