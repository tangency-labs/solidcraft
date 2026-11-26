# curve.curve_p5 (`SmCurve`, part 5)

No Rust-vs-C++ disagreement: 3067 of 3067 non-deferred difftest cases bit-identical (97
upstream-derived and edge-case fixtures: `TestSmCreateRectangle` / `TestSmCreateRegularPolygon`
edges through clean-up, duplicate removal, `IsLine` / `IsLinear` / `IsArc` / `Length` /
`IsDomainBoundary`, the `my_test_reversed_line_join` lines, closest-point inversions, Newton
Jacobians; plus 3000 generated; a 20000-seed run is clean; `--perturb` detects every case).
After wiring `mass.integrator`, 3095 of 3097 cases pass bit-identically. The 28
curved-span `Length` cases now pass; two gap-closing cases still reach
`SmCurve::GetNaturalInterval` through a solution handle (`UNPORTED(curve.curve_p2)`
in `geom::solution_array`).
Upstream behaviour worth knowing:

- **`CrvsCleanup` crashes (SIGSEGV)** when the curve next to a removed tiny curve is closed:
  `TrimGap` between the previous curve and a closed curve (end points equal) dies inside
  upstream (reproduced with a cubic segment, a tiny segment and a closed quadratic loop).
  The port completes the clean-up. Difftest never generates a closed curve next to a tiny
  one. `CrvsCleanup` also casts every neighbour to `SmBSplineCurve` for `TrimGap`; the port
  returns `InvalidInput` for any other kind there and the ffi only sends B-splines.
- **`Length` casts its cache curve to `SmBSplineCurve`** and integrates any curve the cache
  tessellates, Hermite curves included (the Rust cache tessellates them as upstream does),
  so Hermite `Length` uses numerical integration, rather than the end point distance
  the comment promises.
- **`SmFindCCNormalENFO::Evaluate`** reads `m_cpOptVectors[0..1]` without a `NULL` check for
  `SM_SO_SIGNED_PIVOT_MINIMIZE` (the port returns `InvalidInput`), and the iteration count
  that triggers the curvature "step back" only counts evaluations with a Jacobian.
- **`sm_IsPointSolutionAcrossClosedSeamBoundary`** compares the solution's object pointers
  with its curves; the port's caller (`GlobalCoincidenceChecker`) passes whether the first
  object is `this` (the split solutions always name `[this, other]`).
- **`SmCurve::CacheMakeOrValidate`** is reached in the port only through
  `make_curve_cache` (building) and `validate_curve_cache` (type check, reuse);
  `GetOrCreateObjectCache` on an object without the curve returns `InvalidInput` when a
  new curve cache would be needed.
- **Wiring this unit exposed four harness / port gaps in dependent units** (their cases
  were deferred on `curve.curve_p5` before):
  - `SmPolarConversion::ConvertToNURBSParameter` writes the polar-table guess to
    `rdNurbsParameter` before the point inversion; when the inversion fails (`SM_ERR`)
    callers that ignore the status (`SmEllipse::AssertValid`) use that guess. The ellipse
    port now does the same (`convert_to_nurbs_parameter_or_written`); without a usable table
    nothing is written and upstream reads an uninitialised `dNurbT` (UB): `curve.ellipse_p1`
    no longer generates those cases.
  - `SmSolution::IsEqual` scales its parameter tolerance by the object's natural interval;
    the `curve.curve_p3` / `curve.curve_p7` runners' solution handles now are curve objects
    with the curve's interval (as upstream's `SmCurve*`), so near-equal minima merge.
  - `GlobalPointSolve` answers from the cache's analytic curve, whose pointer names the
    solution: `Curve::global_point_solve_step` takes that handle (`analytic`).
  - `SmGlobalSolver::SolveIt` initialises `sStartBranch[i]` only for `i < m_lNumTrees`; a
    one-tree solve whose top node is a leaf hands an uninitialised `apBranch[1]` to
    `LocalSolve`, and `SmCurvePropertyGS::LocalSolve`'s angle special case copies it into
    `m_apNodes[1]` of a one-object solution. The `curve.curve_p7` harness reports the second
    node only for two-object solutions.
- **Cost of rebuilding the curve cache per call (fuzz00504, PR #149, 2026-10-20).** The
  Caches deviation above (`make_curve_cache` builds a fresh cache for every caller, where
  upstream's `CacheMakeOrValidate` keeps it on the curve until the curve changes) is a
  Rust-only slowdown once a caller intersects the same curves repeatedly.
  `offset.offset_geometry_creation --fuzz 2000` (at #149's `ed40f92c`; the same at
  `da779c51`) reports 1 timeout, `fuzz00504`. Its inputs: base `gen0054`, mutations
  `/pairs/0/1/2 = 4294967296` and `/ops/0/curve/cpts/2/0 = 1e+300`; `disk.smb` and
  `sphere.smb`, `dist 100`, `approx 1e-5`, analytics on. The slow op is the fifth,
  `trim_extended_edge` (base `[0,9,12]`, offset `[1,9,25]`).
  - Time for the script up to that op: C++ about 2 s, Rust 18-24 s (the fuzz budget is
    20 s a side). The case completes with no difference; it is slow, not hung.
  - All 5 gdb samples of the Rust side are in one stack: `trim_extended_edge` ->
    `global_curve_intersect` -> `curve::curve::p5::make_curve_cache` ->
    `CurveCache::tessellate` -> `compute_constants` -> `solve_it` -> `evaluate` ->
    `nurbs_crv::crv_derivs` / `basis_derivs_into`.
  - Not related to the exact `SmSurfOfRevolution` import (`ed40f92c`): the op takes 18.8 s
    with it and 22.0 s with it disabled. #149's wiring of the `UNPORTED(topology.brep_p2)`
    stubs made this `TrimExtendedEdge` path reachable for the case; before, the unit's
    `--fuzz 2000` was `FUZZ-PASS`.
  - Not a defect of `offset.offset_geometry_creation` or `topology.brep_p2`, and no guard
    was added (overseer ruling). The fix is a cache kept per curve across calls, as
    `Curve::global_point_solve_cached` does for `GlobalCoincidenceChecker`; it belongs to
    this unit's owners.
  - See also: the "curve.curve_p8 (`SmCurve::GlobalCurveIntersect`): Rust runs for minutes
    where C++ returns at once on a 1e300 control point" section (fuzz01278) and the
    fuzz00405 note in the `offset.offset_geometry_creation` section (`approx = -1` with a
    `-1e300` point, Rust 21 s in this cache's tessellation under `global_curve_intersect`,
    the closest match), both on sc-topo_isect_p1's #156 follow-up branch
    `fix/offset_geometry_creation-review` (bluecmd/solidcore, head `14f66bf4`, not yet a
    PR); and `surface.surface_p8`'s "Cache cost at tiny scale" (HANG-15). fuzz00504 also
    carries a `1e+300` control point (in its first op's curve).
