# blend.fillet_intersector `rv_next_clipped_step`: fixed, the wrong `PointOnPlaneSolve` override (2026-10-23, follows the open entry above)

- **Root cause.** Not the offset-surface extension state. `SmFilletSolver::StepSolve`'s clipped
  branch for a constant-radius-assisted solver calls the virtual `PointOnPlaneSolve`, which
  `SmConstantRadiusAssistedFS` overrides (`SmFilletStandardSolver.cpp:1084`, inherited by
  `SmEiffelTowerFS` and `SmSecantConstantRadiusFS`). In the port, `StepSolve` dispatches on
  `blend::fillet_solver::p1::FilletSolverOps`, and the impls of that trait for
  `ConstantRadiusAssistedFs`, `EiffelTowerFs` and `SecantConstantRadiusFs` did not override
  `point_on_plane_solve`, so the base `SmFilletSolver::PointOnPlaneSolve` ran (only the
  intersector's `FilletSolverOps` trait had the override). For the case the iso-curve
  intersection gives `v = -0.1` on both sides; the override's sphere solve then takes one Newton
  step to `-9.999999999999992e-2` (C++), while the base version returned the guess `-0.1`.
- **How it was pinned.** A local C++ probe in the shim (not committed): the solver's offset
  extensions after the op are the same on both sides (rail 0 extended to `[0,1] x [-1.1,1]`
  with identical control points and knots, rail 1 not extended); a fresh `LocalCurveIntersect`
  on the iso curve gives `t = -0.1` in C++ as in Rust; `PointOnPlaneSolve` on fresh offsets
  gives `-0x1.9999999999994p-4` with or without a prior extension. So the extension history did
  not cause the difference.
- **Fix.** The three `p1::FilletSolverOps` impls now override `point_on_plane_solve` with
  `blend::fillet_standard_solver::p2::point_on_plane_solve`. This also reaches the other callers
  of the virtual through that trait (`sm_FindFilletTsectPnt`, the p4 `IntersectorSolver`,
  `fillet_geom` p2's `RailEdgeuseIntersect` helper).
- **Extension state (also fixed, no effect on this case).** `StepSolve` and
  `SmFindClippedRadiusEFO` evaluated copies of the solver's offsets (the iso curve, the other
  surface for `LocalCurveIntersect`, `EvaluateNormal`, `LoadInitialValues`' `crSurface1`) and
  dropped the extensions those evaluations made. Upstream evaluates the solver's own objects. Each
  copy starts from the solver's current extension and nothing else evaluates that surface in
  between, so after each use the solver's offset now takes the copy's extension
  (`FilletSolver::keep_extension_of_copy`, using `OffsetSurface::take_extension_of`). This gives
  the same history as one shared object. `EvaluateNormal` now runs on the solver's own surface.
  Real sharing is not possible because `IsoCurve` and `Surface` own their surface. PR #658's
  domain-compare sync (`follow_extension_of`) between the intersector's trace copies and the
  solver is unchanged. Unlike that sync, this hand-back is unconditional.
- **Result.** `blend.fillet_intersector` 1292/1292 pass, known-mismatch entry removed. No
  tolerance changed and no floor added.
