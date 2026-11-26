Updates: WR-422

#### WR-422 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Still in v1.0.1: `SmFilletSolver::CalcTorusFilletGeom` (`SmFilletSolver.cpp:860`-`:1233`) is unchanged. Its gate `sm_TestAnalyticSurface` (`:1372`) now takes any `IsKindOf(SmCone_TYPE)` surface (`:1377`), so the cylinders of the record's rounded block reach the torus attempt through `:1981`-`:1982`. `issue_417_torus_center_on_axis` still fails by default and matches the closed form with `fillet_torus_center_on_axis`, on `main` with the v1.0.1 port. Difftest agrees with the C++ 1.0.1 by default (`blend.fillet_solver_p1`..`_p5`: all pass).
