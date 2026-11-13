# blend.fillet_solver_p2: `LoadInitialValues` gets a copy of the rail's offset surface

`PointOnPlaneSolve`, `RailEdgeuseIntersect` and `RailRailIntersect` pass `*GetSurface(lRailIndex)` as `crSurface1` of the
virtual `LoadInitialValues`. p1's `FilletSolverOps::load_initial_values` takes `&mut OffsetSurface`, so p2 passes a clone of the
rail's surface and writes the clone back afterwards (keeping an extension the evaluation grows). A `LoadInitialValues` that also
changes the solver's own rail surface (`SmConstantDistanceFS` calls `SetOffsets(approx radius)` on both rails) would have that
change on the rail overwritten by the write-back, where upstream it sticks (`crSurface1` is the rail surface). The test solver's
`LoadInitialValues` does not touch the rails, so no case shows it. #168's `SmConstantDistanceFS` (all four virtuals ported) needs
the trait's `surface1` to name the rail (as #168's `InitialSurface` does) and a p1 `FilletSolverOps` implementation for
`ConstantDistanceFs`, both in `blend.fillet_solver_p1`'s interface.

Why the harness still runs the test solver (`P2FS` / `TestSolver`) and no library solver (review of PR #249 corrected this note):
- `SmConstantRadiusFS::LoadInitialValues` belongs to `blend.fillet_standard_solver_p2`, so with #168's `SmConstantRadiusFS` every
  solve would defer. `EiffelTowerFs` and `SecantConstantRadiusFs` (`blend.fillet_custom_solver`) do implement `FilletSolverOps`
  (inherited `GetJacobianSize` / `SetupOffsetValues`, their own `LoadJacobian`), and their `LoadInitialValues` returns at once
  without `lDoSurf2Calcs`, so `PointOnPlaneSolve` (which passes 0) and the `Calc*FilletGeom` builders (which only call
  `SetupOffsetValues`) could run on them on both sides; the ops that pass `lDoSurf2Calcs = TRUE` (`RailEdgeuseIntersect`,
  `RailRailIntersect`) reach the unported `GuessPointOnPlaneSolve`, and a `ConstantRadiusAssistedFs`-based solver answers
  `IsKindOf` with `Unported`, which stops `RailRailIntersect` at its `SmTemporaryRadiusChange` class test.
- Not added to this round: a library solver is built from an edge use of a solid B-rep (`SmConstantRadiusAssistedFS(.., pEdgeuse)`
  takes the offset surfaces and orientations of the two faces at the edge), which needs `blend.fillet_custom_solver`'s solid
  fixtures and its C++ constructor plumbing in this shim; the plane and geometry ops would then compare the real
  `FilletSolverOps -> ConstantRadius*` dispatch once. A follow-up for whoever wires `blend.fillet_standard_solver_p2`.
- The test solver reaches what no library solver does: a varying radius through `slope`, 0-5 extra unknowns (the user-double
  overflow of `PointOnPlaneSolve`, UB-192). The mirror is record by record: `TestSolver` (runner) against `P2FS` (shim).
