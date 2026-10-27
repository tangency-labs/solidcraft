Updates: WR-421

#### WR-421 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Still in v1.0.1: `SmFilletSolver::CalcTorusFilletGeom` is `SmFilletSolver.cpp:860`-`:1233` as at c9c5979 and its circular-section test is unchanged. `sm_TestAnalyticSurface` (`:1372`) now takes any `IsKindOf(SmCone_TYPE)` surface (`:1377`), so a cylinder neighbour enters the analytic attempts at `:1981`-`:1982` where it did not before; the chamfer still goes to the offset trace there. `issue_420_chamfer_on_arc` still fails by default (the plate) and matches the closed form with `fillet_chamfer_cone_on_arc`, on `main` with the v1.0.1 port. Difftest agrees with the C++ 1.0.1 by default (`blend.fillet_solver_p1`..`_p5`: all pass).
