# topology.edgeuse_p3 (`CreateUVTrimCurve`, `GetOrCreateUVTrimCurve`, `EvaluateBinormal`, `NormalizedEvaluate`; PR #117; partial)

- **Upstream defect WR-158** (`plan/upstream-issues.md`): the two-singular-end-points recovery of
  `CreateUVTrimCurve` adds its joined curve to the second half's temporary array, so it never yields
  a curve. The port reproduces it.
- **`gen0007` (seed 15768384652651002757), fixed in the harness.** `GetOrCreateUVTrimCurve` of an
  edge use that already has a curve reports the cached maximum edge/face gap's length. A B-rep read
  with `rebuild` has that gap set in the C++ (`1.08e-16`); the graph export carries only whether the
  gap is set, so the Rust record has the unset length (`-SM_UNDEF_DOUBLE`). Not a missing write in
  the port: the length is not exported. Both sides now report `-2` (not compared) for that distance
  when the edge use has a curve and a set gap (also done for `init_gaps` before). The paths that
  compute a new distance are compared.
- **Gap export** carries only the init flags of the cached gaps, never their lengths, so any result
  that reads a cached edge/face gap length of an imported B-rep (`GetOrCreateUVTrimCurve` of an
  existing curve, the `ii == 2` tolerance of `CreateUVTrimCurve`) compares only where the gap is unset.
- **`EvaluateBinormal`'s UV direction (`pOptUVVector`), review item 1: harness gap, not the
  port.** `box_with_large_fillet.smb` edge use 11 (frac 1 / 1.5 / 1e300, `want_uv_vector`,
  both `use_3d`) gave (1, 0) in the C++ and (0, -1) in Rust, 106 failures in `review.json`
  (and a "partial vector" is not what `ComputeParamDirection` leaves:
  `smgu_VecLinCombTwoVectors` writes nothing partial; on failure the out-param is the
  `(0, 0)` it was initialised to, which both sides now compare). Cause: the graph export
  (`sc_edgeuse_graph.h`) gave an `SmPlane` as origin and normal only, and the importer
  rebuilt it with `Plane::from_normal`, whose X/Y axes, UV scale and domain differ from the
  file's plane. Position, normal and the UV point agree (they do not depend on the axes),
  but `ComputeParamDirection` projects the binormal onto `Du`/`Dv`, i.e. onto those axes.
  The export now carries the whole plane (`3 O X Y su sv umin vmin umax vmax`, from
  `GetPosition`, `GetUVScale`, `GetSTEPUVDomain`) and the importer builds `Plane::new`.
  New fixtures `fixtures/topology.edgeuse_p3/review.json` ask for the UV vector on that
  face (the maintainer's fracs, a sweep of 48 edge uses x 2 params x both modes, with and
  without rebuild) and on the first 8 edge uses of every upstream file.
- **`EvaluateBinormal`'s surface-cache side effect** (`smsurf_GetSurfaceCache` building the face's
  trimmed cache, which creates the UV trim curves of the face's edge uses) defers as
  `UNPORTED(topology.face_p3)` (`SmFace::CreateUVTrimCurves`) on the paths that reach the global point
  solve.
- **`query.loop` wired (review item 3).** `query::r#loop::contains_uv_point` no longer reads a
  borrowed `FaceLoop` view: it takes `&mut Brep` and the loop id and calls
  `GetOrCreateUVTrimCurve` in walk order (a failing status or a missing curve gives
  `SM_POC_UNKNOWN`, as upstream), reading the edge interval and tolerance after each creation.
  `query::face::p2::point_classify` and `classify_param_on_faces` (and
  `api_queries::face_classify_uv`) take `&mut Brep` for it; the difftest runners classify on a
  clone. The `query.loop` runner builds the one-face B-rep of the shim's `SmBrepConstructor`
  in Rust (face with surface owner and tolerance, loop, edges, edge uses with their trim curves).
  No `UNPORTED(topology.edgeuse_p3)` stub is left in `crates/`.
- **`topology.proto_topology_p2` stub kept (justified).** `edgeuse_evaluate_binormal` in `topology/proto_topology/p2.rs` (`SmEdgeuse::EvaluateBinormal`, called while re-classifying proto faces of a split face) stays `UNPORTED(topology.edgeuse_p3)`: the proto topology reaches the caller's B-rep only through the `ProtoBrep` trait over opaque handles (the difftest pool is a bare stand-in with no `Brep` and no `Kernel`), so there is nothing to call `topology::edgeuse::p3::evaluate_binormal` on. Wiring it means adding a `ProtoBrep` method and a `Brep`-backed implementation with the owner of that trait (`topology.proto_topology_p2`).
- **`intersect.curve_class_p9` stub kept (justified, found when merging main).** `p9_edgeuse` (`IntersectEdges` on a cached trimmed-surface traversal) stays `UNPORTED(topology.edgeuse_p3)` for an edge use without a UV trim curve (`SmEdgeuse::GetOrCreateUVTrimCurve`). Its boundary views are snapshots of opaque `ClassGeometry` handles built by `query::face::p2::curve_on_classify` from a borrowed `&Brep`, so a curve created there could not be stored in the caller's B-rep or resolved through the handle mapping. Wiring it means `curve_on_classify` taking `&mut Brep` and creating the missing trim curves as the cached traversal reaches them; the path is not reachable end to end yet (`curve_on_classify` first defers on `SmCurveClassification::ReSet`, `intersect.curve_class_p2`).
- **`heal.gap` wired**: `GetGapEndPoints` now creates a missing UV trim curve; the gap constructors,
  setters, `CacheGap` and `GetGapEndPoints` take `k` and `&mut Brep` (and so do
  `vertex_edge_gap`, `calc_vertex_edge_gap`, `edge_ccw_edge_gap`, `calc_edge_ccw_edge_gap`,
  `max_down_dim_gap_3d`).
- **`query.edge` radial classifications wired (`gen0091`, `gen0261` solved: harness gap).** With
  `evaluate_binormal_step_off_into` called from `radial_sector_classify`, two generated cases
  disagreed with the C++: `gen0091` (seed 2056831836502447629; `box_with_large_fillet.smb` read with
  `rebuild`, eu 328290, classify 192469, nparam 0.41016510631606873: cpp result 4, rust 1) and
  `gen0261` (seed 2056831836502447799; same file, edits `p0`, nparam -1e-9). Cause: not the
  port. The `query.edge` shim's private `put_surface` was a copy that predated the `SmPlane` branch of
  `sc_edgeuse_graph.h`; `SmPlane` derives from `SmBSplineSurface`, so it exported the faces' B-spline
  part and the Rust import saw B-spline faces. `RadialSectorClassify` tests `IsKindOf(SmPlane_TYPE)`
  (`bAllPlanes`): true in the C++ (all three faces planes), false in Rust, so the C++ declared
  coincidence at once (`bAllPlanes` and `dAng3 < 0.001` degrees: result 4, `CoincidentRadial`) where
  Rust stepped off four times (count > 2 and `dMinDist > dTolerance`) and returned inside. Every step-off
  call, its distances and its outputs agreed bit for bit (checked by calling
  `EvaluateBinormalStepOff` from the shim at the same distances). Fix: the shim exports an `SmPlane`
  as a whole plane, `1 O X Y su sv umin vmin umax vmax` like `sc_edgeuse_graph.h` (item 5 of the AGENTS.md checklist: the shim builds every geometry kind the code branches
  on). Those faces are now real planes in Rust, so 28 cases that passed on a B-spline stand-in defer on
  the global point solve instead (`surface.surface_point_solve_p3`): query.edge 1368 cases, 1279 pass, 0
  fail, 89 deferred at the time (after merging main: 1316 pass, 0 failed, 52 deferred, all
  `intersect.surface_drop_lib_p3`). A zero-length `Unitize`/`AngleBetween` in the classifications is an `SM_ERR`
  status, as `SER` returns it, not a null dereference; the step-off locals start at
  `SM_UNDEF_DOUBLE` like upstream's default-constructed vectors. Other shim copies of `put_surface`
  without the plane branch (`topology.vertex`, `topology.brep_data`, `topology.proto_topology_p1`,
  `curve.crv_on_surf`, `surface_plane_p1`, `blend_fillet_custom_solver`, `sweep_api_surfaces`) were
  not touched; any of them used by a unit that branches on `IsKindOf(SmPlane_TYPE)` has the same gap.
- **Harness.** The `topology.edgeuse_p2` shim called nothing on edge uses outside a loop (it reported a
  null dereference); `EvaluateBinormal` returns `SM_ERR_INVALID_INPUT` for them without dereferencing,
  so the shim now calls the C++ there (40 cases). The `topology.loopuse` shim runs
  `GetUVCornerBoundaries` of edge loops now (300 cases).
- `IsTangentSector` and `SmEdge::GetBlendEdgeuse` ignore a failing `EvaluateBinormal` (`SE`) and read
  the default-constructed `SM_UNDEF_DOUBLE` locals; the port does the same (the partial writes of a
  failure midway are not modelled).
