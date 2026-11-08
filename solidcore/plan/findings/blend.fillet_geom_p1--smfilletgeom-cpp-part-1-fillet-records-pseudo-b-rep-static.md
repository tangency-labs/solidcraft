# blend.fillet_geom_p1 (`SmFilletGeom.cpp` part 1: fillet records, pseudo B-rep, static helpers; PR #147, 2026-10-16; partial, no disagreements)

Gate: `PARTIAL blend.fillet_geom_p1 cases=480 passed=446 failed=0 deferred=21 max_rel_err=0e0 rejected=13`
(`cases_both_errored` 0, so 446 real passes; the 13 rejected cases are the flagged
regression inputs below; 2000-seed sweep 2051 pass / 116 deferred / 0 failed / 13 rejected; `--perturb` 446/446 detected).

- **Deferred (unported callees, no stub of this unit's own code).**
  `sm_ClassifyFacePoint` calls `SmFace::CalculateUVDomainFromUVTrimCurves`, wired to
  `topology.face_p1` (PR #148) with upstream's defaults (`bLimitDomain = TRUE`, no gap
  outputs) and its status ignored except `Unported`; that port's first callee,
  `SmFace::CreateUVTrimCurves`, is `UNPORTED(topology.face_p3)`, so every face with a
  surface still defers there (then `SmFace::Point3DClassify` of `query.face_p1`):
  14 cases (one is the 1e50 boundary input below), so `SmFilletVertex::UpdateSplitFace` defers only
  for a vertex classified to the split face and other vertices compare.
  `sm_TwoPntsIntersection` defers past its point drops in
  `SmSurface::PointBasedSurfaceIntersect` (5 cases): `surface.surface_p2`'s port stops
  at `SmSurfaceIntersector::DoPointIntersection`, labelled
  `intersect.surface_intersector_p4` although that unit is partial, because part 4's
  function runs the basic intersector's `ComputeSingularityPoint` instead of
  `SmAdvSurfaceIntersector`'s override until the intersector parts dispatch their
  virtual calls (comment in `surface/surface/p2.rs`); cases whose drops fail compare the
  failure. `SmFilletVertex::CalcCliffRailIntEdge` reaches `SmSurface::GlobalCurveIntersect`
  (wired, `surface.surface_p8`), which stops at `intersect.surface_drop_lib_p1`'s own
  unwired call of `SmSurfaceDropCurve::DoTrace` (ported; the stub in
  `intersect/surface_drop_lib/p1.rs` is labelled `intersect.surface_drop_curve`, which
  is what the difftest's `blocked_by` prints): the true owner is
  `intersect.surface_drop_lib_p1`, whose open PR #140 wires it (2 cases: the cliff
  fixture and its near-zero-knot probe; this PR does not build on #140 so its diff
  stays its own).
  `sm_ExamineNFixFilletGeoms` compares only its early returns (analytic/low-degree
  surfaces, no adjustment needed, both surfaces need adjusting, the same geometry
  twice): the adjusting path, control-net copy included, has no compared case. The
  shim's geometries have no `SmFilletSolver` (`new SmFilletGeom(NULL, type)`) and
  upstream dereferences it there (`GetFilletSolver()->GetSurface(k)`: SIGSEGV, found
  by the pre-review), so the shim repeats upstream's degree / kind / end-distance
  pre-checks and refuses (`SC_ERR_ARG`, recorded as a rejected case) any input that
  would reach it; the Rust side on that path rebuilds the rails, takes the drop domain
  from the wired `SmOffsetSurface::GetNaturalUVDomain` (`surface.offset_surface`, #62)
  and stops at `SmSurface::DropCurve` (`intersect.surface_drop_lib_p3`) with
  `Unported`, or with a null-pointer error when, as in the difftest, no solver is
  passed. The Rust adjusting path (net-row copy, rail `SetCurve` resets, the solver's
  offset-distance save and restore, the `Unported` at the drop, the null-solver
  error) is covered by the unit test `examine_copies_the_good_row_rebuilds_the_rail_and_stops_at_the_drop`
  only.
  `sm_FindStepoffUV` and `sm_CreateCubicRailRailInterpolant` take an `SmFilletSolver`
  (unported) and have no difftest cases: the first is ported over the co-ported
  `ConstantRadiusAssistedFs` and a B-rep, the second returns `Unported` at its first
  solver call (`FindIndexOfRailOnFace`); its tail (guess UV point, rail/rail
  intersection, the five-point cubic through `CreateCanonical`) is ported behind the
  two solver call stubs but cannot run yet (lesson 27: a stub replaces only its call).
- **Pseudo-B-rep construction.** `SmFilletEdgeuse(SmFilletBrep*)` needs the pseudo
  B-rep's infinite region and first shell. `SmBrep::GetInfiniteRegion`
  (`topology.brep_p1`, #145) and `SmRegion::GetShells` (`topology.shell`, #154) are
  wired; the region itself is created by `SmBrep()` (`SmBrep.cpp:533`, run by
  `SmFilletBrep()`), whose constructor is `topology.brep_p3`'s and unported, so
  `FilletBrep::new` has none and the constructor stops at
  `UNPORTED(topology.brep_p3) SmBrep::SmBrep` (never upstream's NULL), then at
  `UNPORTED(topology.brep_p2) SmBrep::MakeShellVertexTopology` when the region has no
  shell. The difftest builds the pseudo B-rep with `SmFilletEdge::MakeFilletEdge` on
  the C++ side and with the low-level record writes on the Rust side (both documented
  in the FFI module), so the record methods compare without it.
- **Solution handles.** `GlobalCurveIntersect` and `GlobalPointSolve` record the curve
  and surface they ran on in their `SmSolutionArray`; `Solution::parameter_interval`
  reads the natural domain off that handle. `calc_cliff_rail_int_edge` passes a private
  `CliffHandle` (curve or surface reference) so the natural domains are real, and the
  point drops pass `NoObject` (nothing reads them back).
- **Harness notes (no port issue).** Upstream `CreateWireEdgeFromCurve` glues the two
  vertices of a wire edge shorter than the B-rep tolerance; the Rust-side builder mirrors
  that (a 1e-6-scale target curve maps both its end vertices to one). Upstream
  `CreateFaceFromSurface` refuses surfaces at that scale (the shim then reports
  `SC_ERR_ARG`); generated faces stay at scale 1e-2 or above.
- **Surface cache flags.** `sm_SurfaceDropPoint` / `sm_FindStepoffUV` toggle
  `m_bPointTestEnabled` / `m_bProcessBoundaryCurves` around `GlobalPointSolve`; for a
  standalone surface the cache already has that state, and the Rust point solve of a
  face-owned surface is itself unported (`query.face_p2`), so nothing is toggled.
- **Coverage.** The shim builds plain B-spline surfaces and curves, except for the
  `evaluate_mate_analytic` op (maintainer review): `sm_EvaluateMate` on a real `SmPlane`
  (with and without its NURB) and on a canonical `SmSurfOfRevolution` of a line, so
  both arms of its `SmBSplineSurface` cast compare, inside the domain (the virtual
  `EvaluatePoint`) and outside it (`EvaluatePointOnExtendedSurface`). A real
  `SmCylinder` / `SmCone` cannot be a Rust `Surface` yet (the cone kind's dispatch gap,
  `surface.cone_p1`). Analytic-kind paths inside the other surface callees are never
  compared. `fb` `cliff` (op 31) has 2 status-0 and 2 error-status passes and 2
  deferrals. Ops with no passing case: `classify_face_point` (all deferred), `fb`
  `split_face` of a vertex classified to the split face (deferred; other vertices
  pass), `two_pnts` past its point drops (deferred; drop failures pass), `fb`
  `examine` past its pre-checks (rejected, above), `sm_FindStepoffUV` and
  `sm_CreateCubicRailRailInterpolant` (no cases: they take an `SmFilletSolver`).
  `capping` with a cap (iso-parameter lines lifted through `sm_ExtractIsoCurve`)
  compares.
- **Tolerance classes (maintainer review of `f0240ceb`).** `sm_SurfaceDropPoint` passes
  its `double` dropping tolerance to `GlobalPointSolve`'s `SmZoneTol3d` parameter, which
  converts it (`SmZoneTol3d(double)`, `SmTol.cpp:107`: `SM_USE_DEFAULT` becomes the zone
  tolerance, then `fabs`, `SmTol.h:148`); the port passed it raw, so a negative
  `approx_tol` (e.g. `-1e-3`) found the point in C++ and nothing in Rust (`SM_ERR`):
  5 of the reviewer's 686 adversarial cases (a closed cylinder at `(1, 0, 0.5)`, a
  bilinear corner, cubic patches at scale 1 and 1e-5). Fixed with `zone_tol3d`; the
  audit of every call in this unit's 111 functions found the same raw pass at
  `sm_FindStepoffUV` (`GlobalPointSolve`), `sm_ClassifyFacePoint` (`GetZoneTol3d` of
  the face, an `SmZoneTol3d`), `JoinVertsByCapping` (`LiftCurve`'s `SmApproxTol3d`) and
  the examine path (`DropCurve`'s `SmApproxTol3d`), now converted with `zone_tol3d` /
  `approx_tol3d`. `sm_TwoPntsIntersection`'s `SM_CAST_APPROXTOL3D_PTR(&dSSITol)` is a
  pointer cast (no conversion), so `PointBasedSurfaceIntersect` keeps the raw value;
  `GlobalCurveIntersect`, `SmCurve::GlobalPointSolve` and `ContainsPoint2d` take plain
  `double`s. Regression fixtures `surface_drop_point_negative_tol_*`,
  `two_pnts_negative_tol_*`, `capping_gap_negative_tol` and `capping_gap_use_default_tol`;
  the generator now draws negative tolerances for these ops too.
- **Upstream hang HANG-29 and the magnitude guard.** The `fuzz_guards` sweep found a
  face whose surface has a control point of magnitude 1e154: `sm_ClassifyFacePoint`'s
  C++ path (`CalculateUVDomainFromUVTrimCurves` -> `DropAndTrimCurve` ->
  `SmCurve::GlobalCoincidenceChecker`) splits the boundary forever once squared
  distances overflow to infinity (1e153 returns in 90 ms). That is the only
  non-termination established; below it the call terminates but is slow in a
  shape-dependent way from about 1e15 on (the re-review's probes on the pre-review's
  fixture: one control point of -1e15 in all three coordinates 54 s, -1e20 14 s,
  -1e50 12 s, the same magnitude in one coordinate 0.2 s, both surfaces and the point
  scaled uniformly by 1e30 55 s and by 1e10 3 ms, a 1e-10 weight under a homogeneous
  9.9e39 37 s, -1e80 10 s, -1e100 23 s, -1e120..-1e150 70-99 s; a random sweep of the
  two ops found 14 calls in 40k above 2 s, worst 38 s, all with a value of at least
  3e15, none above 120 s). The FFI therefore bounds, for `ClassifyFacePoint` and
  `FbSplitFace` only (the ops that reach `sm_ClassifyFacePoint`), every value and every
  Euclidean control point `Pw / w` by `MAX_MAGNITUDE = 1e150`, the overflow regime, and
  accepts the slow inputs under it: slow is not undefined, and a lower bound would
  reject valid inputs that are no slower than ones it accepts. The other ops take any
  finite value. Rejected regression cases (`expected_rejection`): the original 1e154
  shape, the pre-review's slow input moved to -1e151, a 1e-120 weight under a
  homogeneous 1e40 (Euclidean 1e160); the -1e50 input is an accepted case (the C++
  returns in 0.3 s). The Rust side still defers that op (on `topology.face_p3`). The
  sweep itself runs one thread per call with a 30 s timeout, 180 s for those two ops
  (`FUZZ_SEED` / `FUZZ_CALLS` override the seed and length); its specials include the
  bound, 1e15, 1e30, the UB-166 knot gaps (both fixup stages) and the HANG-20 bound;
  one mutation sets a value a hair above its predecessor, one compresses a run of
  values towards its predecessor so a knot range drops under the spread tolerance, and
  one sets a run of values to one offset under the scaled zero (an end group). A
  clean sweep is evidence, not proof: the
  earlier sweeps (seeds 777, default, 4242) missed UB-166 and the slow inputs, and the
  widened specials then found HANG-20. With the HANG-20 bound in place, seed 4242 (40k:
  11429 ran), seed 777 (40k: 11490 ran) and the default seed (60k: 17189 ran) found no
  crash or hang.
- **`sm_CreateCubicRailRailInterpolant` asks for degree 2 with cubic knots (unmeasured).** `SmFilletGeom.cpp:4170` calls `SmBSplineCurve::CreateCanonical(crContext, 2, 2, ..)` with 5 control points, knots `[0, 0.5, 1]` and multiplicities `[4, 1, 4]`: 9 knots for 5 points fit degree 3 (the function's name says cubic), not 2. The port passes the same arguments. No case reaches this tail (it takes an `SmFilletSolver`, and `FindIndexOfRailOnFace` / `RailRailIntersect` are stubs), so whether `CreateCanonical` refuses it or builds something else is not measured. Check it in C++ once `blend.fillet_solver_p1/p2` land; it is a candidate for `plan/upstream-issues.md` (round-6 review, minor 2).
- **Fuzz (`difftest --fuzz 2000`, round 6).** The first run found three Rust panics
  (`attempt to add with overflow`, `nurbs/srf_nurb.rs:1087`, `N_SrfIsClosedSmooth`'s
  u-closure loop): a fuzzed surface `q = i64::MAX` that the FFI refuses, but whose
  Rust side the fuzz still runs. `nurbs::srf_basic::Surface::new` and
  `crv_basic::Curve::new` check only shapes ("see `is_valid`"), so the runner now
  applies the FFI's count rules (`1 <= p <= n`, `r = n + p + 1`, likewise in v and
  for curves) before building either. Library code that takes an unvalidated
  `srf_basic::Surface` still assumes consistent counts (owner: `nurbs.srf_nurb`).
  Round 8: fuzz mutations that unflag the HANG-20 fixtures (`expected_rejection =
  false`) reached the Rust solver above the 1e307 bound, where it loops as upstream
  does (3 timeouts). The runner now runs the FFI's own validation
  (`blend_fillet_geom_p1::validate`, which `eval` calls) before the Rust side, so an
  input the FFI refuses is a harness error on both sides and never runs in Rust.
- **Upstream crash UB-166: a near-zero or near-start interior v knot.** The
  re-review's fuzz found a SIGSEGV in `sm_DecomposeSrf` through the surface cache
  (`sm_SurfaceDropPoint`, and `sm_ClassifyFacePoint` through `SmSurface::DropCurve`):
  every `SmBSplineSurface` constructor runs `sm_FixupKnotVector`, whose first stage
  snaps a knot less than `SmTol::GetScaledZero(u0, u1)` (`1e-12 * (1 + max |u|)`, 2e-12
  on `[0, 1]`) above its already-snapped predecessor onto it, and whose second stage
  spreads distinct knots closer than `SM_EFF_ZERO_PARAM` (1.2e-8) to 1.212e-8 apart,
  moving the first knot of the last pair down with nothing keeping it off the start knot
  (the round-3 review's `v = [0, 0, 0, 5e-9, 5e-9, 1.212e-8 x3]` becomes
  `[0, 0, 0, 0, 0, 1.212e-8 x3]`). Either way the start multiplicity reaches
  `degree + 2`, `N_BasisGetSpanCount` does not count the zero-length span but
  `sm_DecomposeSrf`'s v loop extracts a patch for it and then reads one patch pointer
  past the array; the u loop has upstream's B262 guard and fails with status 1006
  instead. A third route (round-4 review): an end group within the scaled zero of its
  predecessor (`[.., E, E + d x3]`, `E = 1.212e-8`) snaps to `E` first, and the spread
  then moves the knot before it onto the start. A Rust replica of both stages in the FFI
  missed that route by one operation (it took the unsnapped last knot), so the guard
  now lives in the shim: after the real constructor, `read_surface` reads the knot
  vector back and, for the six ops whose upstream path builds the surface cache
  (`SurfaceDropPoint`, `TwoPnts`, `ClassifyFacePoint`, `FbSplitFace`, `FbVertOnVert`
  through `SmSurfaceIntersector::DoIntersection`, `FbCliff` through
  `SmSurface::GlobalCurveIntersect`), refuses (`SC_ERR_ARG`) a surface whose v start
  multiplicity exceeds `degree + 1`. That condition is exact by construction, but it is
  the crash precondition rather than a per-path prediction: the round-5 review put every
  surface of 169 cache-op programs at start multiplicity `q + 2` with the guard
  bypassed; 141 crashed in `sm_DecomposeSrf` and 28 completed because their path
  returns before building the cache for that surface (`TwoPnts` 4 with 1001,
  `FbVertOnVert` 9, `FbCliff` 2, `FbSplitFace` 13; valgrind showed nothing new on 10 of
  them, and the Rust ran all 10). Those 28 are refused anyway; telling them apart would
  mean replicating upstream's control flow. Every other op, the
  u over-multiplicity and curves are compared as given (the round-3 review ran the
  snapped inputs through every other op without a crash). The rejected regression
  cases are the reviewers' reproducers: `*_near_zero_v_knot*_rejected`,
  `surface_drop_point_spread_v_knot_rejected` and
  `surface_drop_point_endsnap_v_knot_rejected`.
  Non-monotone constructed vectors (the spread moved a knot below the start) are
  defined in C++ (valgrind clean, round-4 review) and compared:
  `..._spread_v_knot_non_monotone` (1006 on both sides), `..._spread_v_knot_below_start`
  (1001) and `..._endsnap_v_knot_control` (status 0, one solution). Comparing them found
  three port bugs on the surface-cache path, fixed here as faithful-port fixes in
  units already on main (`surface.surface_p1`, `nurbs.nurbs_srf`,
  `surface.surface_cache_p3`): (1) `surface::surface::p1::make_surface_cache` /
  `make_trim_srf_cache` stored `surface.clone()`, which for a B-spline is the copy
  constructor and runs `sm_FixupKnotVector` a second time, where upstream's cache keeps
  a pointer to the surface itself (`m_cpSurface`); on an already non-monotone vector the
  second fixup snapped the below-start knot back onto the start and the bounded
  decomposition refused it; they now store `clone_for_evaluation()`. (2)
  `nurbs::nurbs_srf::compute_net_constants` returned the error of the UV diagonal
  sampling's `N_SrfEvalPt`, whose status upstream ignores (gdb: 18 `N_BasisEval`
  numerical errors inside `sm_ComputeNetConstants` during that cache build, and the
  build completes); the port now keeps upstream's function-scope points and leaves them
  unchanged on a failed evaluation. (3) `surface::surface_cache::p3`'s
  `subdivide_node` returned the error of `split_srf` on a patch with a reversed domain,
  where upstream's `GW_ERR(sm_SplitSrf(..))` only logs: `N_SrfSplit` fails in its
  parameter checks before writing, so the children keep the zero-filled
  `SmMemBlockMgr` memory (`calloc`'d blocks, elements handed out sequentially) that
  `sm_InitNurbSurfaceMemory` sized like the parent; the port now uses `zeroed_like` for
  both, as its approximation branch already does. With the three fixes the three
  non-monotone cases compare bit-identical (`below_start` 1001, `non_monotone` 1006,
  `endsnap_v_knot_control` status 0 with its solution). Two other `Clone`s stand for upstream using the
  same object and would differ the same way on such vectors, but no case here reaches
  them: `SurfaceCache::map_objects` (`surface/surface_cache/p1.rs`) and
  `out_of_bounds_copy` (`surface/surface/p3.rs`); noted for their owners. The
  u-direction transpose (`surface_drop_point_near_zero_u_knot`) is a compared case:
  both sides 1006, once `surface_drop_point` ported upstream's own
  `smsurf_GetSurfaceCache` + `NER` fetch (a surface whose cache cannot be built is
  `SM_ERR_NULL_POINTER` before any solve; the Rust solve builds the cache again, which
  costs time only). Curves:
  `sm_CreateBezSegments` skips a snapped start over-multiplicity and bounds its segment
  index, and their constructors pass `SM_EFF_ZERO` as the spread tolerance, so curves are
  only checked as given; the probes `fb_cliff_near_zero_curve_knot` (a 1e-13 knot
  inserted in the cliff's target and edge curves) and `capping_near_zero_uv_knot` (in
  both UV curves) run on the C++ side without a crash.
- **Upstream hang HANG-20: the global solvers in the top decade of the double range.**
  This round's fuzz, with the widened specials, hung three ops: `FbCliff` (seed 4242),
  `SurfaceDropPoint` (seed 777) and `TwoPnts` (seed 4242), each on a program with an
  `NL_NOW` marker (`-DBL_MAX`) or `-7e307` dropped into a surface coordinate:
  `SmGlobalSolver::SolveBranch` with the curve/surface, point/surface or
  surface/surface local solver never returns (gdb). Replaying each program with that
  coordinate substituted (`replay_program`, `REPLAY_FILE` / `REPLAY_SET`): the cliff
  returns in 3 ms at `-7e307` and never at `-8e307` or `+-DBL_MAX` in any coordinate; the
  point drop returns at `-3e307` and never at `-7e307`; the two-point intersection
  returns at a homogeneous `-2e307` (Euclidean `1.3e307`) and never at `-3e307`; all
  return in 1-3 ms at `-1e300` and below (`-1e200`, `-1e150`, `-1e100`). So the trigger
  is the coordinate itself overflowing by a small factor in the solvers' box arithmetic,
  not HANG-29's squared distances. The FFI bounds every value (Euclidean `Pw / w`) of
  those four ops (`FbVertOnVert` drops a point too) by `MAX_SOLVER_MAGNITUDE = 1e307`,
  under every measured hang and at or under every measured return; the other ops are
  untouched (capping, the seam checks and the record ops ran the same values without
  hanging in every sweep). Regression cases: `*_hang20_*_rejected` (the three fuzz
  programs and the cliff at `-8e307`, flagged) and `*_hang20_boundary_1e307` (the same
  programs at `-1e307`, accepted: the point drop and two-point cases compare, the cliff
  one compares its `SM_ERR`).
- **Rejected cases are opt-in.** A fixture carries `expected_rejection: true` when the
  wrapper or the shim must refuse it; only then is a refusal recorded as rejected
  (`Output::rejected`, as `intersect.curve_class_p6` does with `fixture_rejected`). A
  refusal of any other case, generated ones included, is a harness error, as is a
  flagged case that runs, so a guard that over-rejects fails the gate.
- **Record writes without a notification environment.** `SmVertex::SetPoint` and
  `SmEdge::SetCurve(.., TRUE)` notify `SM_NO_CHANGE_GEOMETRY`; the pseudo B-rep has no
  notification environment, so the port applies the same resets directly (vertex-use
  gaps; edge-use UV trim curve, drop-curve record and edge/face gap with its mate's,
  and their vertex-use gaps). The co-ported `SmFilletEdge` constructor sets the edge
  tolerance to `0.0` as `SmEdge()` does. `SmBoundaryTrimmingType` is also co-ported,
  identically, by PR #168 (`blend.fillet_standard_solver_p1`); whichever lands second
  re-exports the other's (COORD.md).
- Upstream defect UB-159 (`SmFilletGeom` constructor leaves `m_dOrientations` and
  `m_bCapped` uninitialised) recorded in `plan/upstream-issues.md`.
