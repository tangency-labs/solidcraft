Updates: WR-444

#### WR-444 (update, 2026-11-16). `point_solve_keeps_triangle_guess` is removed

`SurfPtGlobalSolver::find_polygon_guess` always keeps the suspicious control-net triangle guess as a candidate. It returns that guess when it is nearer on the surface (`CalcFuncVal`) than the nearest control point's guess. The guess stays suspicious, so the grid search still runs. The fix is now unconditional and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::point_solve_keeps_triangle_guess`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it (#14246, group B surface-generic).

- **Results:** goldens rows of `surface.surface_point_solve_p2` (28), `surface.surface_p3`, `surface.surface_p4`, `query.api_queries`, `blend.fillet_geom_p4` and `intersect.surface_drop_lib_p2` move. The pull request's class table gives the certificate for each row.
  - In all 28 `point_solve_p2` rows (rough and polygon guesses), replaying `CalcFuncVal` at the old and new guesses gives new <= old. The new guess is strictly better wherever it moved.
- **Tests:** `surface::surface_point_solve::p2::tests::a_point_solve_finds_the_point_on_stacked_runs` (was `..._with_the_fix`) runs on a default kernel.
