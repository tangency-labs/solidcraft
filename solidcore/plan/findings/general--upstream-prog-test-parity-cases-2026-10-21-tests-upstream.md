# Upstream prog_test parity cases (2026-10-21, `tests/upstream-parity`): what they found

New fixtures replicate upstream prog_test calls in `surface.surface_p2` (`ssi_adv.json`,
`smAdvancedTest.cpp` `my_test_ssi_adv`), `curve.curve_p1/p7/p8/p9`, `surface.surface_p7/p8`,
`surface.surface_p11` and `intersect.surface_drop_lib_p1` (`upstream_prog_test.json`).

- **`surface.surface_p2` SSI: tangent and singular start points dropped.** Eight
  `my_test_ssi_adv` part files (`ssi_tor_sph_tan`, `ssi_tor_sph_tan2`,
  `ssi_tor_in_tor_tang_hyper`, `ssi_tor_in_tor_tan_ellip`, `ssi_rev_rev_sing`,
  `ssi_cyl_cyl_sing2`, `ssi_clasic_ruled`, `ssi_cylcyl_90_tan`) disagree on `main`: Rust finds
  0 or fewer curves (e.g. `ssi_tor_sph_tan`: C++ 4 boundary start points, Rust 0). Cause:
  `SurfaceIntersector::compute_point_values` (`intersect/surface_intersector/p2.rs`, the
  near-parallel-normals branch) calls the base `compute_singularity_point_base` (`SM_ERR`)
  where upstream's virtual call reaches `SmAdvSurfaceIntersector::ComputeSingularityPoint`
  (`SmAdvSurfaceIntersector.cpp:116`), so `FindBoundaryStartPoints` skips those points and
  traces stop at them. PR #140 adds this dispatch (`advanced` flag); on #140's head all 56
  pairs pass or defer (0 failures). The eight files are back in the fixture (all 56 pairs;
  on #140's head at `c70653a63` they pass, 545 to 5679 values each), so this unit fails on
  `main` until #140's dispatch lands.
- **`surface.surface_p11` `LiftCurve` `PS_lift_crv2` on `PS_lift_srf2`: fixed** (case
  `lift_crv2_kinked_uv_trim_curve` added): `sm_IntersectAndSplit`'s plane intersection
  solutions named no object, so `IdenticalSolutions` merged only within `100 * SM_EFF_ZERO`
  instead of 1/10000th of the curve's natural interval and a duplicate split at the UV
  curve's C0 knot 13 survived; `intersect_and_split` now takes the curve's handle.
- **Solutions naming no object (or an object without its interval): audit.** Upstream's
  solutions always name the solve's curves / surfaces, and `IdenticalSolutions`
  (`SmGlobalSolver.cpp:1865`) and `SmSolution::IsEqual` merge within 1/10000th of the
  object's `GetParameterInterval` (only the first object's kind is tested,
  `GetObjectIndexForParameterIndex`, so a NULL first object such as the point of the SP
  solver or the line of the LSI solver collapses to `100 * SM_EFF_ZERO` upstream too). A
  sweep instrumented `--green` for merges decided without an interval. Fixed:
  - `Curve::global_curve_intersect`, `global_curve_solve`, `global_property_analysis` and
    `global_point_solve` with no handles now solve naming `OperandObject` stand-ins
    (natural intervals; one object for a self intersection) and return the solutions
    without objects. This covers every `..::default()` caller: `sm_IntersectCurveAndSplit`
    (`surface/surface/p2.rs`; fixture `isect_curve_split_kinked.json` in `surface.surface_p2`:
    six `PS_lift_crv2` kinks cut by 2D lines, 5 of which failed before: one split more or less than C++,
    e.g. `kink17_w3_v` a duplicate split), `surface/surface/p8.rs` `boundary_intersections` (CoincidenceCheck),
    `nurbs/b_spline_curve/p3.rs` `global_property_analysis` (AssertValid, hit ~1600 times by
    `nurbs.b_spline_curve_p3` / `curve.composite_curve_p1` without changing an output),
    `intersect/surface_drop_curve.rs` (break points), `blend/fillet_geom/p4.rs` (needs
    exactly 1 intersection), `topology/brep/p2.rs` `uv_curve_hit`, `surface/plane/p1.rs`
    CoincidenceCheck, `topology/proto_topology/p3.rs`, `intersect/curve_class/p5.rs`,
    `surface/surf_of_revolution/p2.rs` (step inversion). Without handles `sm_RefineCCSolutions`
    also resolved both objects to this curve.
  - `Surface::drop_point_fast` (surface of revolution, `this` was `None`) and the seam
    solutions of `GlobalPointSolve_1` (`AddSortedSolution`, `m_pSrf`) name a surface stand-in
    with the natural UV domain.
  - Handles reporting no interval/domain, which made a near-duplicate pair `Unported`
    (`SmCurve::GetNaturalInterval`, labelled `curve.curve_p2`): `GapCurve` (TrimGap,
    `nurbs/b_spline_curve/p3.rs`), `PlaneHandle` (`curve/curve/p4.rs` HasPlaneCrossings),
    `BoundaryObject` plane (`intersect/curve_class/p8.rs`), `Side`
    (`intersect/curve_class/p9.rs`), `SupportHandle` (`blend/fillet_solver/p1.rs`). 21 deferred
    cases now pass: `curve.curve_p4` 13, `nurbs.b_spline_curve_p3` 6, `curve.curve_p5` 2 (all
    three now green).
  Checked and fine: `SolutionRequested::Single` callers that read only the best solution
  (`offset_geometry_creation.rs:2361`, `fillet_standard_solver/p1.rs`, `fillet_custom_solver.rs`,
  `fillet_geom/p1.rs:2002`, `topology/brep/p1.rs` `find_side_of_curve`); emptiness-only
  readers (`sweep/trans_sweep_geometry.rs`, `blend/fillet_executive/p2.rs`); the SP
  (`GlobalPointSolve_0`) and LSI solvers, whose first object is NULL upstream too; coincidence
  checker callers (`[None; 2]`: no merging); `analytic_handle(s): None` next to real handles
  (`blend/hcr.rs:1414`, `offset_geometry_creation.rs`, `boolean/topology_intersector/p1.rs`:
  the analytic results are never merged by interval); the plane `DropPointFast` (one
  solution). Not fixed (a near-duplicate pair is `Unported`, never silently different):
  surface handles without a UV domain in curve/surface and surface/surface solves,
  `boolean/merge/p4/arena_pair.rs` `H` (`K_FACE_SURFACE`) and the generic handles of
  `intersect/topology_solver`; `surface_intersector/p4.rs` `SolveSurface` runs a nodes-only
  solve. Runner handles are not library code (`surface.surface_p9`'s `Handle` reports `[0, 1]`
  for every curve and no surface domain).
- **Upstream assertion that the C++ itself fails** (`surface.surface_p8`,
  `smSurfaceTest.cpp` `my_test_csi_tangency`): for the lines with y < 1 the test asserts 2
  hits, but C++ returns one range-of-values solution (e.g. `prog_csi_tangency_line_00`: t
  0.4776 to 0.5224); Rust returns the same, so the cases are kept. The circle cases match the
  assertions.
- **Back-edges that now block these cases although their units are partial** (library
  wiring, not done here): `intersect/surface_drop_lib/p1.rs` `drop_curve_trace`
  (`UNPORTED(intersect.surface_drop_curve)`; defers the 5 `PS10_drp` DropAndTrimCurve cases;
  `SurfaceDropCurve::drop_trace` exists), and `Surface::create_silhouette_curves`
  (`surface/surface/p1.rs`, `UNPORTED(intersect.surface_tracer)`; `SurfaceSilhouette` and its
  trace exist; the non-perspective post-processing of `SmSurface.cpp` ~9632-9695 is also
  missing), which keeps `smAdvancedTest.cpp` `my_test_sil_adv` out of reach.
- **`curve.curve_p1` fuzz: `Curve::tessellate` allocated from `min_segments` unchecked**
  (`tess` with `minseg = 4294967296` aborted the Rust side): fixed on `main` by PR #322
  (`Line::tessellate` reserves with `try_reserve_exact`, `InvalidInput`).
