# blend.fillet_geom_p5: examine solvers

`SmFilletEdge::CalcCrossSection` (PR #264) calls `sm_ExamineNFixFilletGeoms` for the two
fillet geometries of an Nx2 corner or a closed blending corner. Its port in
`blend.fillet_geom_p1` (PR #147, `examine_n_fix_fillet_geoms`) takes the geometries'
solvers as co-ported `ConstantRadiusAssistedFs` values, while the fillet-geometry routines
of `blend.fillet_geom_p4` / `_p5` resolve `GetFilletSolver()` in a table of
`blend.fillet_solver_p1`'s `FilletSolver` base (PR #192). There is no conversion from the
base to the derived solver, so the call is an `UNPORTED(blend.fillet_geom_p1)` stub marked
`justified:` with this section. It is reached only past `SmFilletCorner::GetCornerType`
(`UNPORTED(blend.fillet_corner_p1)`), so no difftest case reaches it; wire it once p1's
signature takes the `FilletSolver` base (or a solver trait) and the corner unit lands. No
disagreement.
