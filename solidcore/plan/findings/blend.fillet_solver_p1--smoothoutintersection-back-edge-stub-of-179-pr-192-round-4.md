# blend.fillet_solver_p1: SmoothOutIntersection (back-edge stub of #179, PR #192 round 4)

`blend.fillet_intersector` (#179) listed ten back-edge stubs tagged `blend.fillet_solver_p1`; this PR wires them in
`impl FilletSolverOps for ConstantRadiusAssistedFs` (`blend/fillet_intersector.rs`) except the one below.

- **`SmSelfIntersectionHandler::SmoothOutIntersection` (`FlushCurve` step 9, `SmFilletIntersector.cpp:1569`) cannot be wired by
  this unit and is owned by no unit.** The call is virtual on the handler object `GetSelfIntersectionHandler()` returns; the
  only override that exists is `SmMakeCurveBlendSIH::SmoothOutIntersection` (`SmFilletSolver.cpp:8157-8669`, 513 lines), which
  `harness/callgraph/units.json` does not assign to any unit (it appears in `unused_functions.txt`: the call graph cannot resolve
  the virtual call, and nothing calls it directly). This unit owns only the handlers' constructors/destructors (no data).
  The handler is the member `SmFilletExecutive::m_pSelfIntersectionHandler`: a solver outside an executive has none (upstream
  dereferences the NULL `m_pExecutive`), and `FilletSolver::self_intersection_handler` already reports the executive's data as
  `Unported` (`blend.fillet_executive_p1`), so `SmoothOutIntersection` is unreachable from the Rust solver before that unit lands.
  It is therefore an explicit `Unported` override tagged with the unit that must create the handler
  (`blend.fillet_executive_p1`); whoever ports the executive should also port the `SmMakeCurveBlendSIH` class
  (`SmoothOutIntersection`, 513 lines; the `CreateBlends` / `SmMakeSurfaceBlendSIH` overrides are dummies or unused).
- **`m_pFSG->CreateSurface`** is wired through `FilletSurfaceGenerator::create_surface`: the base class gives `SM_ERR`, the Eiffel tower
  generator its surface, the linear/circular/blend-curve generators stay `Unported` in `blend.fillet_standard_solver_p1/_p2`. The
  generators read only the rails, the fillet points and the approximation tolerance, so the centre line, the UV curves and the two
  surfaces the C++ call also passes are not needed.
- **`ComputeSurfaceValues` on `ConstantRadiusAssistedFs`** is wired to `FilletSolver::compute_surface_values`, which calls the virtual
  `SetupOffsetValues`; the `SmConstantRadiusFS` override belongs to `blend.fillet_standard_solver_p1`, so the call ends in that
  unit's `Unported` until it lands (deferred, as before, in the intersector's difftest).
