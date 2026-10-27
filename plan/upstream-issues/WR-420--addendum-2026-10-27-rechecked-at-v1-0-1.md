Updates: WR-420

#### WR-420 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Still in v1.0.1: the analytic attempts end with `CalcTorusFilletGeom` at `SmFilletSolver.cpp:2000` (same line as c9c5979) and there is no `CalcSphereFilletGeom`. What changed is the gate of those attempts: `sm_TestAnalyticSurface` (`:1372`) now accepts any `IsKindOf(SmCone_TYPE)` surface (`:1377`), so a cylinder counts as analytic; it is used at `:1981`-`:1982`. The record's trigger has cylinders (the bottom rounds), and `issue_416_fillet_seam` still shows the failure by default and the closed-form volume with `fillet_corner_ball_patch`, on `main` with the v1.0.1 port. Difftest agrees with the C++ 1.0.1 by default (`blend.fillet_solver_p1`..`_p5`, `blend.fillet_executive_p1`, `_p2`: all pass; `_p3` is partial with 4 known mismatches, not examined here).
