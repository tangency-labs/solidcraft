# blend.fillet_geom_p4 (`SmFilletGeom.cpp` part 4: `sm_CreateUVCurve`, the fillet-surface intersection vertex calculators, `GetRailIndexOfVertex`, `MakeFaceBrep`, `InsertIntersectionTopology`, `CalcBlendingGeom`; PR #252, 2026-10-19; partial)

- **Built on open PRs.** The fillet object model (`FilletBrep`, `FilletGeom`, ...) is
  `blend.fillet_geom_p1` (PR #147) and the solver is `blend.fillet_solver_p1` (PR #192);
  both are merged into the branch (with #242's reconciliation commit `ec075471`), so this
  PR lands after them. The difftest runner reuses p1's model block on both sides: the
  shim is `harness/cpp/shim/blend_fillet_geom_p4_static.inc`, included at the end of
  `blend_fillet_geom_p1.cpp` (the only translation unit compiling upstream
  `SmFilletGeom.cpp`, whose statics it calls), and p1's runner/FFI helpers are
  `pub(crate)`.
- **What compares.** `sm_CreateUVCurve` fully (seam pair with and without a cut corner,
  iso lines, both-on-seam pairs, degenerate-edge drops, the minimise fallback, the
  vertex-use branch, B-spline / real `SmPlane` / real `SmSurfOfRevolution` surfaces,
  1e-6..1e6 scales): bit-identical. The vertex calculators, `MakeFaceBrep` and
  `CalcBlendingGeom` compare up to their first unported callee
  (`sm_SrfSrfIntersection`, `JoinTwoFilletVertsByCurves`, `CalcBlendingRail`);
  `GetRailIndexOfVertex` only for a NULL vertex (`GetRailIndexOfVertexuse` is p6's);
  `InsertIntersectionTopology` has unit tests only (it needs a real
  `SmFilletExecutive`). The block's fillet corners are opaque tags, so the shim refuses
  runs that would call into a corner (and solver/rail dereferences the block cannot
  satisfy); see the FFI module docs.
- **Coverage gap, to be closed by later porters.** Only `sm_CreateUVCurve` is compared
  on real values. `MakeFaceBrep`, `CalcBlendingGeom` and the three vertex calculators
  are compared only on early exits: everything after `JoinTwoFilletVertsByCurves`
  (`blend.fillet_geom_p6`) or `sm_SrfSrfIntersection` (`blend.fillet_geom_p2`) is
  deferred, and the p1 model cannot express an original face on a fillet edge, so
  `CalcRailIntExtendedEdgeuse` never gets past `NER(pSideFace)`. The porters of p2 and
  p6 should extend the runner (and the p1 model) to reach those paths. Once original
  faces are expressible, the shim must also refuse `CalcRailIntExtendedEdgeuse` on a
  vertex *without* a corner that has a side face (upstream then dereferences `NULL`
  `m_cpCorner` in `GetExtendedSurface`).
- **Review fixes (found by reading the C++; not reachable by difftest yet).**
  (1) `SM_CAST_PTR(SmBSplineCurve, ..)` also succeeds for `SmLine`, `SmEllipse`/`SmCircle`,
  `SmHyperbola` and `SmParabola` (all derive from `SmBSplineCurve`); the first port
  accepted only `CurveKind::BSpline`, so a line or circle rail/side curve would have been
  `SM_ERR_NULL_POINTER` in `MakeFaceBrep` and `CalcBlendingGeom`. (2)
  `InsertIntersectionTopology` keeps one `aDeletedEdges` for the whole routine
  (`MergeCurveOnSurfaces` appends, `SmTopologyIntersector.cpp:3879`); the first port
  passed only the current merge's edges to `ClearFilletBrepEdges`, leaving stale edge ids
  on later rails/side edges. (3) `CalcFilletInt2Fillets` now checks the fillet surfaces
  before resolving the solvers, as upstream (a geometry with neither returns
  `SM_SUCCESS`). 20 adversarial `sm_CreateUVCurve` cases (seam pairs with negative and
  zero tolerances, cut corners, swapped revolution parameters, cone apexes, huge/tiny
  planes, coincident points) were added as fixtures; all bit-identical.
- **Second review fixes (found by reading the C++; not reachable by difftest yet).**
  (1) `sm_CalcNx2VertFilletXFillet` passes its two fillet surfaces straight to
  `sm_SrfSrfIntersection`, which starts with `NER(pSurf1); NER(pSurf2)`: a `NULL` surface
  (fewer than two cross-section edges, or a geometry without a fillet surface yet) is a
  failed intersection, so the vertex becomes `SM_FIL_SURF_INT_FAILURE` and the call
  returns `SM_ERR`. The first port returned `SM_ERR_NULL_POINTER` before the call and
  left the vertex status alone (unit test `nx2_without_fillet_surfaces_is_surf_int_failure`;
  the shim refuses vertices with a corner, so difftest cannot reach it). (2) `MakeFaceBrep`
  holds the solver tolerance in an `SmApproxTol3d` (`USE_DEFAULT` -> zone/2, then `fabs`)
  before using it as `d3dTol` and in `SmTol::MapTo2d`; the port used the raw value, so a
  negative or `USE_DEFAULT` solver tolerance gave other trim and UV tolerances. Six
  `make_face` fixtures with solver tolerances `-1e-6`, `-0.05` and `USE_DEFAULT` were
  added; they defer at `JoinTwoFilletVertsByCurves` (`blend.fillet_geom_p6`) today and
  will compare once p6 lands. (3) Smaller: `pPrimEU->GetMate()` and the UV curves of a
  successful `sm_SrfSrfIntersection` are now dereferenced only where upstream does (the
  final `SetUVCurve`, the rail branch of the corner blend, the reversal);
  `InsertIntersectionTopology` passes a `NULL` fillet surface on to `RegisterExistingSSI`
  instead of failing early; `sSSITol *= 10.0` in `sm_CreateUVCurve` is a plain multiply
  (`tol_mul`). The corner blend's base surfaces with no B-spline net becoming `None` is
  now in the Deviations line. 22 more adversarial `sm_CreateUVCurve` cases (spindle and
  fan B-spline patches pinched at an edge at three scales, flat patches with zero, huge
  and negative tolerances and points outside the domain) were added; all bit-identical.
- **Disagreement outside this unit (fuzz, `--fuzz 30000`, 2 notes).**
  `SmSurface::GlobalPointSolve(SM_SO_INTERSECT)` on a real `SmPlane` *without* its NURB
  (`MakeNurb` not called) whose origin is very far from the test point (origin
  `x = 1e50..1e300`, UV scales ~1e-3..1e-6, test point near the global origin) returns
  `SM_ERR` (1001) from `SmSurfPtGlobalSolver::Solve` (`surface.surface_point_solve_p2`,
  reached after `SmPlane::DropPointFast` finds nothing within tolerance), while the Rust
  solve returns `Ok` with no solutions; `sm_CreateUVCurve` then goes on (minimise drop,
  both points clamp to the same corner) and fails at `CreateLineSegment` with
  `SM_ERR_INVALID_INPUT` (1007). Inputs: `uv_curve` on plane `origin (1e50,
  -4.04e-05, -9.60e-04)`, `x_axis (1, 0.2307, 0)`, `y_axis (0.1478, 1, -0.1387)`,
  `uv_scale (1.687e-3, 1.262e-3)`, domain `[-0.327, 0.358] x [-0.610, -0]`, no NURB,
  points `(6.39e-4, -8.65e-4, -8.53e-4)`, `(6.48e-4, -5.48e-4, -8.98e-4)`, tol `1e-11`.
  The same plane with origin `1e10`, or with its NURB made, agrees. The bounding box
  (and so the `1e-9 * size` drop tolerance `1.0217e-12`) is identical on both sides
  (checked under gdb), so the difference is inside the surface point solver's handling
  of an un-NURBed plane at that magnitude. Not fixed here (green unit
  `surface.surface_point_solve_p2`); the generator's planes stay within `1e6`.
- **Two `FilletSolverOps` traits after merging main (2026-10-19).** `blend.fillet_intersector`
  (#179, merged) co-ported a `FilletSolverOps` in `blend::fillet_intersector`, and
  `blend.fillet_solver_p1` (#192, since merged) has its own in
  `blend::fillet_solver::p1`. Both traits still exist on main (2026-10-20), and the
  intersector still carries `UNPORTED(blend.fillet_solver_p1)` stubs although that unit
  is now `partial`; unifying the traits and wiring those stubs is left to a follow-up.
