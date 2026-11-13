# blend.fillet_solver_p1: one type per class after #168 (merged main after #168/#263, 2026-10-20)

#168 (`blend.fillet_standard_solver_p1`) co-ported `SmFilletSolver` members (`FilletSolverData`), `SmFilletSolverType`,
`SmFilletSurfaceGeneratorType`, `SmFilletSolver::IsKindOf` and the `SmFilletSphereSolveENFO` members, all owned by this unit, and
ported the generator classes `SmLinear/Circular/BlendCurveCrossSectionFSG` whose constructors this unit owns. After the merge there is
one Rust type per C++ class: the constant-distance, curve-based and variable-radius solvers hold this unit's `FilletSolver` (their
`GetSurface` goes through `FilletSolver::surface`, a private `SolverRails` helper dereferences it); `FilletSolverType`,
`FilletSurfaceGeneratorType` are re-exported from here; `FilletSolver::is_kind_of` is `SmFilletSolver::IsKindOf`;
`GuessSphereSolve` wraps `FilletSphereSolveEnfo` (it now takes the solver, as upstream's constructor does); and
`FilletSurfaceGenerator::Circular` / `BlendCurve` hold #168's `CircularCrossSectionFsg` / `BlendCurveCrossSectionFsg` (continuity is
`u32`, as #168 chose). `ConstantRadiusAssistedFs` (custom solver) stays #168's `ConstantRadiusFs` as before. `SmFilStatus`
had three copies (`blend.fillet_geom_p1`, this unit, `blend.fillet_intersector`); the owner's (`fillet_geom::p1::FilStatus`) is
re-exported by the other two, so the intersector's `SetStatus` no longer maps between enums. Not merged: the intersector's
`FilletSolverOps` trait (the `SmFilletSolver` virtuals and members `SmFilletIntersector` calls, from #179) and this unit's
`FilletSolverOps` are still two traits; `ConstantRadiusAssistedFs` implements both, the intersector's by delegating to the base.

Wired with it: the `SmConstantRadiusFS` / `SmConstantRadiusAssistedFS` overrides of `ConstantRadiusAssistedFs`, `EiffelTowerFs` and
`SecantConstantRadiusFs` (`GetJacobianSize`, `SetupOffsetValues`, `OffsetRadiiCanChange`, `GetType`, and `LoadJacobian` of
`ConstantRadiusAssistedFs` itself); `IsKindOf` and `LoadInitialValues` stay explicit `Unported` overrides (`_p2`).
`SmLinearCrossSectionFSG::CreateSurface` in `FilletSurfaceGenerator::create_surface`; `SmFilletSolver::SetExtensionFlags` in
`VariableRadiusFs::set_extension_flags` (which now takes the B-rep and stops at the corner lookup, `blend.fillet_executive_p1`); and
`SmCircularCrossSectionFSG::CreateSurfaceFromCurves` in `offset/offset_geometry_creation.rs`. Dispatch note for the last one:
upstream's `rCenterLine` is an `SmBSplineCurve&` whose virtuals (`IsLine`, `IsArc`) dispatch on the real kind (an `SmLine` or
`SmEllipse` centre line); #168's `create_surface_from_curves` takes the NURB part as a plain `BSplineCurve`, so an analytic centre line is
tested with the B-spline versions. Not hit by a compared case yet.
