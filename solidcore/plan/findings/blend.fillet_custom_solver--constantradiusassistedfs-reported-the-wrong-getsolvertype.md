# blend.fillet_custom_solver: `ConstantRadiusAssistedFs` reported the wrong `GetSolverType` (2026-10-22)

Found while landing `blend.fillet_solver_p4` (#212, batch/finish-3). `impl FilletSolverOps
for ConstantRadiusAssistedFs` returned `SM_FS_CONST_RADIUS`, with a comment saying the
intersector shim builds an `SmConstantRadiusFS`. It does not: `IxCra` in
`harness/cpp/shim/blend_fillet_intersector.cpp` derives from `SmConstantRadiusAssistedFS`,
whose `GetSolverType` is `SM_FS_CONST_RADIUS_ASSISTED` (`SmFilletStandardSolver.h:112`).
`StepSolve` branches on the type. This did not show until `ComputePointValues`
(`blend.fillet_solver_p4`) was wired: the intersector's `ComputeNextPoint` cases then reached
`StepSolve`, and `blend.fillet_intersector` gen0180 disagreed on the next point (Rust
`SM_IP_TANGENT_POINT`, C++ crossing). Fixed to upstream's value. The `blend.fillet_solver_p4`
runner's `ShimSolver` models a plain `SmConstantRadiusFS`, so it now overrides `GetType`,
`GetSolverType`, `IsKindOf` and forwards `LoadInitialValues`. The fix changes no other unit's
counts.
