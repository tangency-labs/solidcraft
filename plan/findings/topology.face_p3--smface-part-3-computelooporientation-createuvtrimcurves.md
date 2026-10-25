# topology.face_p3 (`SmFace` part 3: `ComputeLoopOrientation`, `CreateUVTrimCurves`, `FaceIntersect`, `IsDegenerate`, `CreateComposites`, `SetTolerance`, `Notify`, lamina tests, PR #139, 2026-10-16; partial, no disagreements)

- **Coverage.** Every case that runs on both sides agrees bit for bit (1758 cases, 530
  pass, 1228 deferred). What compares values: the three lamina queries
  (`HasConsecutiveLaminaEdges` defers on `SmFace::GetEdges(.., TRUE)`,
  `topology.face_p4`), `SetTolerance` (every face, edge and vertex tolerance), `Notify`
  `PRE_EDIT` / `ADD_TO_BREP` / `RM_FROM_BREP` (rectangular-trim flag and every edge use /
  vertex use state), `ComputeLoopOrientation` of an inner vertex loop only (32 cases on
  faces built with `SmBrep::MakeVertexLoop`: the early return of
  `SmFace.cpp:1704`-`1708`, orientation and bounds), `IsDegenerate` with a zone tolerance
  above every edge (all edges degenerate: the early `TRUE`; the 56 passes compare 2 values
  each) and `FaceIntersect` of non-coincident face pairs whose surfaces do not meet (the 70
  passes compare 4 values each: status, curve count, point count and the empty
  arrays). **Nothing compares for `CreateComposites`** (all 68 cases defer at
  `SmEdgeuse::NormalizedEvaluate`, `topology.edgeuse_p3`), for `ComputeLoopOrientation` of
  an edge loop (all deferred there, `sm_AreConnected` / `GetOrCreateUVTrimCurve` /
  `NormalizedEvaluate`; checked on a rebuilt `plane_square.smb` that the C++ takes the UV
  path there too, so the deferral hides no disagreement), for `CreateUVTrimCurves` (it
  defers at `GetSingularities` of the face-owned surface, the `SmFace::GetTolerance` stub
  of `surface.surface_p3` tagged `topology.face_p2`, or at `GetUpwardLoopuses`,
  `topology.face_p4`) or for the rest of `IsDegenerate` (the `SmCurveClassification`
  constructor, `intersect.curve_class_p4`) and `FaceIntersect` (`CoincidenceCheck`,
  `surface.surface_p8`, or the classification of its curves). The `CreateUVTrimCurves`
  runs with the `own_curves` argument (the shim and the runner hand copies of the face's
  UV trim curves with their edge use's orientation, after clearing them, so
  `sm_FindBestUVCurve`, `CheckAndSetUVTrimCurve` and the `SmCrvOnSurf_TYPE` test of
  `SmFace.cpp:8117`-`8160` are exercised) defer the same way, so those paths are not
  compared yet; they start comparing when `topology.face_p4` lands.
  **Update (review round 2026-10-18, `topology.edgeuse_p3` wired).** `ComputeLoopOrientation`
  of edge loops now compares in the 3D walk (orientation, bounds and use states, e.g. the
  closed B-spline cylinder face, whose `sm_CheckSeamCrossing` period shift runs, 43-51
  values per case); its UV path defers inside `GetOrCreateUVTrimCurve`
  (`SmSurface::DropCurve`, `intersect.surface_drop_lib_p3`) or at a `SmFace::GetTolerance`
  stub tagged `topology.face_p2` (144 cases, on the `.smb` plane faces). `CreateUVTrimCurves` no longer stops at `GetSingularities`
  (it uses `get_singularities_of_face`): it defers at `GetUpwardLoopuses`
  (`topology.face_p4`), so the pole-gap test and the `SmCrvOnSurf_TYPE` branch still wait
  for `topology.face_p4`. `CreateComposites` defers inside `CreateCurvesFromFace`
  (`SmSurface::DropCurve`).
- **Import / dispatch gap (not a pending port).** The `surface.surface_p2` and
  `surface.surf_of_extrusion_p2` deferrals come from the runner's `inexact_geometry`: the
  graph import keeps the C++ type codes 15012 (`SmCone`), 15013 (`SmCylinder`), 15014
  (`SmSphere`), 15015 (`SmTorus`) and 15030 (`SmSurfOfExtrusion`) only as their B-spline
  part, so a geometric op on such a B-rep defers. The kinds' own units
  (`surface.cone_p1`, `surface.sphere`, `surface.torus`) are green, so the deferrals name
  the owner of the missing `SurfaceKind` variants, the `SmSurface` dispatch of
  `surface.surface_p2` (issue #76; the DISPATCH GAP of `Surface::read_from_db`), and
  `surface.surf_of_extrusion_p2` for the extrusion (its own `SurfaceKind` gap). They clear
  when those variants exist and the import builds them (until PR #139's review round they
  named the green kind units, so they could never clear).
- **Harness.** The B-rep blocks, graph export and import are those of the
  `topology.vertexuse` runner (planes as `SmPlane`, lines as `SmLine`). Its import keeps
  analytic `SmBSplineSurface` / `SmBSplineCurve` subclasses (cone, cylinder, sphere,
  torus, revolution, extrusion, ellipse) only as their B-spline parts, so the geometric
  ops (`LoopOrient` onwards) on such B-reps defer to the unit that gives the kind a Rust
  representation (the mapping of the `topology.edgeuse_p2` runner). The shim's own op 0
  exports each face's surface type and each edge's curve type for that test. The shim
  outputs `-2` instead of calling a geometric op when the target face (or, for
  `FaceIntersect`, either face) has no surface or one of its loops' edges has no curve
  (upstream dereferences them; the ops touch nothing else); the Rust side does the same.
  `ComputeLoopOrientation` checks the selected loop instead, which may lie on another
  face: a vertex loop returns before any geometry (never `-2`, and compared on every
  B-rep); an edge loop needs the target face's surface and its own edges' curves and,
  without `bDo3DLoopComputation`, the surface and all edge curves of the loop's own face
  (a missing UV trim curve is created through that face's trimmed surface cache, whose
  `CreateUVTrimCurves` reads them). Until PR #139's review round the check covered only
  the target face's edges, so a loop of another face with a curve-less edge reached
  upstream's null curve dereference.
  Block kind 4 (a kind-2 face plus an inner vertex loop at given fractions of its
  domain, `SmBrep::MakeVertexLoop`) was added to the shared fixture for this unit.
  `SmallBox.smb` (and `Platform.smb`) do not build upstream and are not used.
- **`CreateUVTrimCurves` order.** Upstream reads `GetUVDomain`, `GetSingularities` and
  `GetNaturalUVDomain` before `SmRescueSurfaceOwner` clears the surface owner; the port
  does the same, so `GetSingularities` sees the face-owned surface (its approximation
  tolerance is the face's).
- **Stubs left for this unit** (`prepush.sh stubs` CHECK, `justified:`):
  `query.poly` `PolyBrep::face_set_original_face` (`SmPolyFace::SetOriginalFace`
  notifies the original `SmFace` with `SM_NO_COPY`; the poly B-rep holds the original face
  as an opaque `ObjectKey`, with no B-rep and no topology record for
  `SmTopology::Notify` to copy from) and `surface.trim_srf_cache`
  `face_create_uv_trim_curves` (the trimmed surface cache holds its face as an opaque
  handle `O`, and the preceding `SmBrep::m_bEditingEnabled` stub, `topology.brep_p1`,
  returns first). Both need a B-rep handle in those types. The
  `MarkFaceContainment` dispatch note of `surface.trim_srf_cache` above still applies once
  that call is wired. Also `topology.face_p1` `set_surface`
  (`Notify(SM_NO_CHANGE_GEOMETRY, pSurface, SM_NO_GET_BREP(this), m_pSurface)`): the
  notification's data arguments are the new and the old surface, and a face-owned
  surface has no object identity in the B-rep model to form them from (the other ten
  `SmFace::Notify` calls of `topology.face_p1` are wired, with the empty environment of
  `topology.loopuse`'s `face_notify_edit`). Also `topology.brep_p6` `make_face_topology`
  (merge with main, #169): its `SmFace::SetTolerance(GetTolerance())` is where that unit
  stops (its header: "face construction at `SetTolerance` (`face_p3`)"); everything after
  it upstream (primary face use, face list and shell insertion, loops, edge uses, radial
  sectors, trims, ~500 lines of `SmBrep::MakeFaceTopology`) is that unit's own unported
  remainder, so calling the ported `topology::face::p3::set_tolerance` there would only
  move the stop into code `topology.face_p3` does not own. **Wired** (the
  `MakeFaceTopology` remainder PR, 2026-10-21): `topology.brep_p6` now ports the whole
  function and calls `topology::face::p3::set_tolerance(GetTolerance(), TRUE, TRUE)`.
  Also `topology.edgeuse_p3` `face_surface_cache` (merge with main, #117):
  `EvaluateBinormal`'s `smsurf_GetSurfaceCache` builds the face surface's trimmed cache,
  whose constructor calls `CreateUVTrimCurves` (`SmTrimSrfCache.cpp:209`); that is the
  `surface.trim_srf_cache` call above, with the same opaque face handle. Building the
  Rust cache there instead (tried in this round) runs without the side effect: 251
  `topology.edgeuse_p3` binormal cases then fail on the edge use states (C++ 44 values,
  Rust 33: the UV trim curves the cache's `CreateUVTrimCurves` attaches are missing).
  **Wired** (PR fix/surface-cache-face-trim, 2026-10-22): `EvaluateBinormal` already
  holds `&mut Brep`, so it builds the face's trimmed cache with
  `query::face::p2::face_surface_cache` (the `BrepCacheFace` adapter, whose
  `create_uv_trim_curves` is `topology::face::p3::create_uv_trim_curves`), keeping the
  side effect; the cache itself is dropped (no object caches in the port, so it is
  rebuilt on each call that reaches it). Tiny adversarial fixture surfaces then exceed
  the port's `MAX_SUBDIVISION_NODES` bound (surface.surface_cache_p3 above) where
  upstream subdivides on; the difftest defers those cases to `surface.surface_cache_p3`.
  Also `topology.brep_p1` `face_intersect` (merge with main, #145):
  `SmBrep::BrepIntersect` intersects a face of this B-rep with a face of ANOTHER B-rep.
  `topology::face::p3::face_intersect` takes both faces from one `&Brep`: its
  classification objects (`IntersectObject::Surface(FaceId)`) and the edge/point
  classifiers address a face by arena id inside one B-rep, so a face of a second B-rep
  cannot be named (the `topology_intersector` adapter likewise refuses faces of different
  B-reps). Wired when the face intersection is generalised to two B-reps.
- **Wired in the review round (merge with main, #117).** This unit's own
  `NormalizedEvaluate` and `GetOrCreateUVTrimCurve` calls go to `topology::edgeuse::p3`
  (`pdOptDistToSurf` only where upstream passes it), and `CreateUVTrimCurves`
  reads the poles with `get_singularities_of_face` (the surface is face-owned there, so
  the face tolerance is the approximation tolerance), no longer reaching the
  `topology.face_p2` `SmFace::GetTolerance` stub of `surface.surface_p3`.
- **Wired elsewhere (merge with main, #148 / #146).** `topology.face_p1` (`CreateUVTrimCurves`;
  ten of its eleven `SmFace::Notify` calls), `geom.tol` (`SmFace::SetTolerance` in
  `UpdateObjectTolerance`) and `boolean.topology_intersector_p1` (`SmFace::FaceIntersect`
  through the new `IntersectorBrep::face_intersect`, which hands the curves and
  classifications of `topology::face::p3::face_intersect` out under the runner's handles;
  `CurveClassification::map_objects` / `PointClassification::map_objects` change the handle
  type).
- **Wired elsewhere.** `topology.vertex` (`RefreshTolerance` -> `SmFace::SetTolerance`;
  the vertex shim now exports each face's tolerance, which the cascade reads; 76 cases
  that deferred now pass), `topology.trimming_tools` (`CreateUVTrimCurves`,
  `ComputeLoopOrientation`; the 219 `topology.face_p3` deferrals now defer inside them),
  `topology.loopuse` (`FlipLoopOrientation`'s `PRE_EDIT` / `POST_EDIT` notifications run
  with an empty attribute store and topology records, which is all a B-rep face carries;
  the 366 deferrals move to `GetUpwardEdgeuses`, `topology.face_p4`),
  `heal.heal_data_p2` (`SetTolerance`, `IsDegenerate`; `Cache_DegenFaces` now takes the
  B-rep mutably, since `IsDegenerate`'s quick check may build UV trim curves).
- **Probe (review round 2026-10-18, `--fuzz 2000`).** No crash and no panic; 0 differs
  after two harness fixes: `SetTolerance` / `IsDegenerate` take an `SmZoneTol3d`, so the
  shim's double is converted (`USE_DEFAULT`, then `fabs`) and the runner now does the same
  with `geom::tol::zone_tol3d` (negative tolerances had differed; regression cases
  `*_set_tol_negative*`, `triangle_pole_degenerate_negative_tol`), and the shared
  `topology.vertexuse` block encoder returns a harness error for an empty knot vector
  instead of overflowing. Two timeouts remain, both C++ (no result after 20 s) on a
  B-spline face with one control point coordinate mutated to `±1e300`: `FaceIntersect` of
  `biquadratic` with itself (coincidence TRUE) and `CreateComposites` of the closed
  cylinder face. Not investigated further (slow or a hang); a magnitude bound in the shared
  block validator of `topology.vertexuse` would reject them, but that guard is shared by
  every runner using the blocks and was not changed in this round.
  Merge round 2026-10-19 (`--fuzz 2000` after allowing `ang_tol_deg == 0` for
  `Intersect`): 0 crashes, 0 panics, 0 differs, 3 timeouts, all on the same `±1e300`
  control point (`Degenerate`, `UvTrim`, `SetTol` on such a face; none is an `Intersect`).
  The generator now draws the angle tolerance from `{0, 0.5, 5, 30}` and the fixture adds
  7 `*_intersect_ang0*` cases; no `Intersect` with a zero angle tolerance hung or
  crashed the C++: upstream only converts it to radians (`SmFace.cpp:11342`) and passes
  it to the surface/surface intersector, so the guard now accepts `[0, 180]`.
- **Upstream defects.** UB-189 (`ComputeLoopOrientation` reads the last of an empty
  edge use array), UB-193 (`FaceIntersect` hands out deleted curves through
  `pOptCC3DCrvs` without classification arrays). `sVeryFirstUV` is defined (the
  `SmVector2d` default initialisers) and ported as such.
