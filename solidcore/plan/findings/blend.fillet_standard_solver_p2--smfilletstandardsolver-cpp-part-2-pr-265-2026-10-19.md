# blend.fillet_standard_solver_p2 (`SmFilletStandardSolver.cpp` part 2, PR #265, 2026-10-19, continued in PR #439, 2026-10-21; partial)

Difftest (PR #439, on main with #236): 405 cases (400 generated + 5 fixtures): 373 pass, 0
failed, 27 deferred, 5 rejected; `--perturb` 373/373; `--fuzz 2000` 0 differs. (PR #265 before the
`ReCalcFilletGeom` port: 388 pass, 17 deferred; 3000 seeds 2893 pass, 0 failed, 112
deferred, max_rel_err 3.8e-16.)

- **Resolved disagreement.** `SmConstantRadiusFS::LoadInitialValues` on an
  `SmSurfaceSurfaceFS` (fixture `fx_a`, generated case `gen0180`, seed `7089733659915066011`;
  `fx_d_guess`): rail 1, the other rail a bicubic x linear B-spline patch offset by `0.391`,
  extension factor `0.1`. The other rail's v guess was `2.3112246736941722e-1` (C++) against
  `2.3103818605745594e-1` (Rust). Traced with prints on both sides: the
  `rpSrf2Offset->LocalPointSolve` in the grown domain `[-0.1, 1.1]^2` leaves the base domain,
  so its first evaluation there extends the base (`sm_ExtendSurface`, G1 ruled, to
  `[-1.1, 1] x [0, 1]`; the extension's points agree bit for bit). Upstream's `const`
  evaluators store it in `m_pExtendedSurface` (`SmOffsetSurface.cpp:710-713`, `:1064-1068`),
  so from then on the offset's `GetNaturalUVDomain()` is the extended one. The solve ends on
  the grown `u = -0.1` boundary; the read that differed is most likely the boundary step of
  `LocalPointSolve_1`, which clamps the iso parameter into the surface's natural domain
  (`surface_point_solve/p3.rs`): `-0.1` upstream, `0` in Rust, whose `&self` evaluators
  dropped the extension after each call (the domain stayed `[0, 1]^2`).
  Fix in `surface.offset_surface`: the `&self` evaluators keep the extension they make in a
  pending slot (a `Cell`) that supersedes the stored one (natural domain, knots, later
  evaluations, copies) until a `&mut self` method folds it in; `offset_local_point_solve`
  hands an extension made on its copy back to the solver's offset. Factor `0` (no
  extension) and `0.5` (the solve stays inside the base domain) were unaffected. The same
  fix resolved the point-on-plane case `gen0386` once `SmFilletSphereSolveENFO::Evaluate`
  was wired. `blend.fillet_custom_solver`, `blend.fillet_standard_solver_p1` and
  `blend.fillet_solver_p1` are unchanged by it (0 failed).
- **ReCalcFilletGeom (ported, PR #439).** `VariableRadiusFs::re_calc_fillet_geom` takes the
  pseudo B-rep its fillet geometry lives in (as `blend.fillet_solver_p1` does since #303) and
  ports the whole routine: the rail walk over the first geometry's rails (unprocessed, mate
  and rail-x-extended-edge-use vertices skipped), the projection of the rail-end points on
  the reference curve and their sorted, de-duplicated insertion, `SetupOffsetExtension` and
  `SmFilletIntersector::DoLawIntersection`, whose points are written back to the vertex
  uses. `DoLawIntersection` evaluates the law first (`SmFilletLaw::Evaluate`,
  `UNPORTED(blend.fillet_executive_p1)`), so a case with exactly two points defers there;
  0, 1, 3 or 4 points (`SM_ERR`), a missing reference curve or rail (`SM_ERR_NULL_POINTER`)
  and a zero point tangent (`Unitize`, `SM_ERR_INVALID_INPUT`) compare. The call runs on the
  base intersector: `DISPATCH GAP` for `SmFilletIntersector`'s overrides (the
  `blend.fillet_intersector` "Virtual overrides" note above). Harness: the `recalc` op
  (shim `blend_fillet_standard_solver_p2_static.inc`, included by
  `blend_fillet_geom_p1.cpp` to build the geometry with its `Model`) constructs a real
  `SmVariableRadiusFS`, pins its offsets and replaces its first geometry by the Model's.
- **Upstream solvers start with one fillet geometry.** `SmFilletSolver()` adds
  `new SmFilletGeom(this)` to `m_vFilletGeoms` (`SmFilletSolver.cpp:65`), so after any
  derived constructor `m_vFilletGeoms.GetSize() == 1`; Rust's `FilletSolver::new` leaves it
  out (`FilletSolver::new_in` / `add_constructor_fillet_geom` add it, #303). A shim that
  `Add`s a geometry to a constructed solver puts it at index 1, not 0 (the first `recalc`
  harness did, and every case read the constructor's rail-less geometry: `NER(pRail)`).
- **StepSolve (stub kept, justified).** `SmSurfaceSurfaceFS::CalcFilletGeom` calls
  `SmFilletSolver::StepSolve` (`blend.fillet_solver_p3`, partial since #236).
  `FilletSolver::step_solve` dispatches on the solver's virtuals through
  `fillet_solver::p1::FilletSolverOps`, which `SurfaceSurfaceFs` does not implement yet
  (its `LoadJacobian`, `LoadInitialValues`, `GetJacobianSize`, `SetupOffsetValues`,
  `SetupOffsetExtension` override and `GetSolverType`). Wiring it needs that impl; it would
  add no compared case now: right after `StepSolve` the tail calls
  `SmFilletExecutive::GetPseudoBrep` (`UNPORTED(blend.fillet_executive_p1)`), and the `ss`
  shim runs `CalcFilletGeom` only to its parallel-normal exit (past it upstream dereferences
  the solver's `NULL` executive). Follow-up with the executive port.
- **FinishConstruction (stub kept, justified).** The `SmConstantDistanceFS` and
  `SmVariableRadiusFS` constructors (and upstream's `SmConstantRadiusFS` surface
  constructor under `SmSurfaceSurfaceFS`) end with `FinishConstruction()`. Because of the
  constructor's default geometry above, upstream it is not a no-op: it installs copies of
  the two offsets into that geometry (`SmFilletGeom::SetOffsetSurface`). It changes nothing
  on the solver, and nothing these units port reads that geometry's offsets. Wiring it needs
  the constructors to take the pseudo B-rep and the solver's id (as `FilletSolver::new_in`)
  and to add the default geometry first; `blend.fillet_standard_solver_p1`'s
  `SmConstantRadiusFS` constructors, which these build on, do not take them yet either. The
  three sites keep `UNPORTED(blend.fillet_geom_p1)` comments pointing here.
- **FinishConstruction (wired, follow-up to #439; supersedes "stub kept, justified" and
  the Rust half of "Upstream solvers start with one fillet geometry" above).** Rust's
  derived constructors now add the `SmFilletSolver` constructor's default geometry
  (`FilletSolver::new_in` / `CurveBasedFs::new_in`; they take the pseudo B-rep and the
  solver's `FilletSolverId`) and call `FilletSolver::finish_construction` where upstream
  calls `FinishConstruction()`: both `SmConstantRadiusFS` constructors (so also
  `SmSurfaceSurfaceFS` and the assisted / custom solvers' edge-use constructor),
  `SmConstantDistanceFS` and the edge-use `SmVariableRadiusFS`
  (`SmFilletStandardSolver.cpp:67, 153, 1315, 1972`), not on their tangent-face early
  returns. The surface `SmVariableRadiusFS` constructor (`:1995`) never calls it upstream,
  so its stub was wrong to name it; it gets the default geometry without copies. The three
  `UNPORTED(blend.fillet_geom_p1)` sites are gone. `FilletSolver::new` and
  `ConstantRadiusAssistedFs::from_parts` still leave the geometry out (bare solvers of tests
  and harnesses). A Rust construction that fails after the base leaves its default geometry
  in the pseudo B-rep (the arena does not delete; upstream's half-built solver holds it).
  Harness: the `SmConstantRadiusFS` edge-use op (`blend.fillet_standard_solver_p1`), the
  `SmConstantDistanceFS` edge-use op and the `Ss` op (`_p2`) also compare that geometry
  (`GetFilletGeoms` size, its solver, and per rail whether it holds its own offset copy,
  with that copy's distance, base identity and a point or its natural domain); case counts
  unchanged, 0 failed.
- **Stacked on #303 (2026-10-21).** PR #265 was closed unmerged and continued in PR #439,
  which merges PR #303 (tdamsma, `blend.fillet_solver_p1` glue: it carries #192 and
  replaces `FilletGeomRef` by `FilletGeomId` with the pseudo B-rep threaded through
  `FilletSolverOps`) instead of #192; #265's WIP also carried froglemonade2's #192 commit
  `f676c2e8`, which #303 lacks. The custom solvers' intersector adapters gained #303's
  four geometry setters (delegating to the base).
- **Stacked on #192 (2026-10-20).** The PR merges PR #192 (`blend.fillet_solver_p1`):
  one `FilletSolver` base, `FilletSurfaceGenerator::Circular`/`BlendCurve` wrapping
  p1's generator structs (continuity `u32`), `ConstantRadiusAssistedFs`'s
  `FilletSolverOps` (whose `IsKindOf` / `LoadInitialValues` this unit wires).
  `PointOnPlaneSolve` dispatches on the solver's class through that trait;
  `blend.fillet_intersector`'s `FilletSolverOps::point_on_plane_solve` is a required
  method (as in PR #249) implemented for `ConstantRadiusAssistedFs`, `EiffelTowerFs`
  and `SecantConstantRadiusFs`. `GuessPointOnPlaneSolve` runs on the solver itself
  (`&mut`), so its extensions stay on the solver's offsets.
- **Executive adapter stubs (kept, justified).** `blend.fillet_executive_p3`'s
  `ExecutiveAccess` (`fillet_executive/p3.rs`) names this unit at four places: the
  `SmConstantDistanceFS`, `SmVariableRadiusFS` and `SmSurfaceSurfaceFS` constructors in the
  default `new_fillet_solver`, and the `SmSurfaceSurfaceFS::CalcFilletGeom` dispatch note on
  `calc_fillet_geom`. The trait does not own the solvers: a handle `O` names an object in the
  adapter's own arena, and the only constructor the trait delegates is
  `new_constant_radius_solver` (`constant_radius_from_edgeuse` on the adapter's B-rep).
  The three constructors need the same arena slot, B-rep access and solver enum, which exist
  in neither the library nor the two runners' adapters (`blend_fillet_executive_p2.rs` keeps
  constant-radius solvers only; `blend_fillet_executive_p3.rs` keeps none). The library
  half is ported here: `ConstantDistanceFs::from_edgeuse`, `VariableRadiusFs::from_edgeuse`,
  `SurfaceSurfaceFs::new` and `SurfaceSurfaceFs::calc_fillet_geom`; an adapter that stores
  these solvers calls them. Wiring needs a solver arena in the executive adapters, which
  is the work of `blend.fillet_executive_p2` / `_p3`'s follow-up, not of this unit.
- Fixtures `fx_*` in `fixtures/blend.fillet_standard_solver_p2/cases.json` keep the inputs.
