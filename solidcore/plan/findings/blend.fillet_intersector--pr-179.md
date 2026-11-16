# blend.fillet_intersector (PR #179)

- **FlushCurve with two or more points cannot run standalone in C++.** Past the span
  equalisation it calls `SmFilletSolver::GetSelfIntersectionHandler`, which reads
  `m_pExecutive` (never initialised by the `SmFilletSolver` constructor, only set inside an
  `SmFilletExecutive`), then writes the rails of `GetLastFilletGeom()`, whose
  `SmFilletGeom(solver)` constructor leaves `m_vRails` `NULL`. The FFI therefore rejects a
  `FlushCurve` of more than one point; the Rust side evaluates the offset rails, stops at
  `GetSelfIntersectionHandler` (`blend.fillet_solver_p1`) and is covered by unit tests. The 0/1-point paths
  (empty list `SM_ERR_NULL_POINTER`, boundary-touch flag, list cleared) are compared.
- **Concrete span mapping.** The Surface integration follow-up removes the
  B-spline-only `SmCrvOnSurf` gate from `TestSpanAccuracy` / `ComputeNextPoint`.
  Hermite UV spans retain the actual surface and its analytic/offset evaluation
  overrides instead of substituting a B-spline part. This does not complete the
  separately deferred solver and executive paths below.
- **Solver calls.** Everything behind `StepSolve`, `ComputePointValues`,
  `ComputeSurfaceValues` and `PointOnPlaneSolve` defers until `blend.fillet_solver_p1..p4`
  implement `FilletSolverOps`; the C++ side still runs these cases against a real
  `SmConstantRadiusFS` (no crash in 6000 generated cases).
- **StepSolve is in/out (review fix).** `SmFilletSolver::StepSolve` takes `double& rdStep`
  and rescales a clipped step into it (`SmFilletSolver.cpp:5228`, `:5402`); `ComputeNextPoint`
  then uses the rescaled step for `rNextTSP.m_dCurveParameter`, the Hermite UV spans, the
  `1000*SM_EFF_ZERO` angle-reject threshold and the `sDiff / dStep` of the through-point
  derivative fix. Its flags are set at entry (`rbClipped = TRUE` before later `SER` exits)
  and copied even when it fails. `FilletSolverOps::step_solve` therefore takes
  `step_size: &mut f64` and `flags: &mut StepSolve` out-params; implementers
  (`blend.fillet_solver_p3`) must write both as upstream does. Fixture
  `rv_next_clipped_step` (step 0.5 from 0.9, domain clipped at u=1) pins the C++ answer:
  next `curve_parameter = 1.0` with `clipped = 1` (deferred until StepSolve is ported).
- **Virtual dispatch gap (for the solver porters).** `SmFilletIntersector` overrides
  `ComputeNextPoint`, `FlushCurve`, `ComputePointValues` and `TestSpanAccuracy`; the Rust
  port has them as inherent methods only, so `TraceCurve` / `DoPointIntersection` /
  `DoLawIntersection` (`intersect.surface_intersector_p4`) run on a `FilletIntersector`'s base
  still call the basic intersector's versions. Before a solver unit traces a fillet through
  those drivers, `intersect::surface_intersector` needs a hook trait the fillet intersector
  implements. *Update (`blend.fillet_solver_p4`, PR #212):* `DoPointIntersection` (with
  `TraceCurve` and `TestPoint`) now runs through
  `intersect::surface_intersector::p4::IntersectorVirtuals`;
  `FilletIntersector::do_point_intersection` dispatches to the overrides
  (`FilletDispatch`). `DoLawIntersection` still has the gap.
- **Surface aliasing.** Every upstream caller (`SmFilletSolver.cpp:2037`,
  `SmFilletStandardSolver.cpp:2273`/`:4721`, `SmFilletGeom.cpp:11627`) builds the intersector
  over the solver's own `SmOffsetSurface`s, and `TestSpanAccuracy` / `ComputeNextPoint` rely on
  `SetSurfacesToZeroOffset` changing them. The Rust intersector borrows separate `&Surface`s,
  so zeroing the solver's offsets does not reach it; the shim and runner also build both sides
  over the base surfaces, so the offset-surface configuration is not compared. Solver porters
  must pass the surfaces deliberately (or add a solver-backed surface accessor).
  *Update (`blend.fillet_solver_p4`, PR #212):* `CalcFilletGeom` traces copies of the
  solver's offset surfaces and keeps their distances in step with the solver's
  (`OffsetSurface::set_offset_distance` takes `&self`, the distance is a `Cell`): on entry
  to `DoPointIntersection` and after every solver call the intersector makes.
- **FlushCrv guard kept.** The FFI still rejects `sm_FlushCrv` inputs whose polygon does not
  have `3 (knots - 1) + 1` points or whose knots are not strictly increasing:
  `CreateCanonical` then indexes NLIB arrays from the polygon size and the expanded knot
  vector, which are out of bounds or degenerate (multiplicity above degree + 1); whether
  NLIB tolerates the repeated-knot case was not established, so those `CreateCanonical`
  error paths are not compared.
- **Aliased points.** `ComputeNextPoint` with `prev == next` returns no point for a
  non-crossing point (as upstream, which returns before touching either reference) and
  `InvalidInput` for a crossing (upstream would read and write one object at once).
