# blend.fillet_solver_p2: back-edges into the solver from blend.fillet_geom

`RailRailIntersect`, `PointOnPlaneSolve`, `RailEdgeuseIntersect` and `CalcCone/TorusFilletGeom` were stubbed in
`blend.fillet_geom_p1` (`sm_CreateCubicRailRailInterpolant`), `blend.fillet_geom_p2` (`SmFilletVertex::CalcRailIntRail`,
`CalcVertOnCrossSection`, `CalcSetBackVert`), `blend.fillet_geom_p3` (`SmFilletGeom::ReCalcFilletGeom`), `blend.fillet_geom_p6`
(`SmFilletGeom::RailEdgeuseIntersect`) and `blend.fillet_intersector` (the `FilletSolverOps::point_on_plane_solve` default).
Wired as follows (PR #466; PR #249's round 4 wired the first three against a `&mut [ConstantRadiusAssistedFs]` table, which
PR #466 replaced after main's #247/#264 built `blend.fillet_geom_p3`..`_p6` on `&mut [FilletSolver]` tables):
- **Solver class.** The `blend.fillet_geom` solver tables hold base `FilletSolver`s, and every geometry shim
  (`blend_fillet_geom_p3.cpp`, `_p4_static.inc`, `_p6.cpp`) builds a base `SmFilletSolver` subclass with only the base
  virtuals. The wired calls therefore run on the table's `FilletSolver` with the base virtuals (`impl FilletSolverOps for
  FilletSolver`, p1): `RailRailIntersect`'s callees and the virtual `PointOnPlaneSolve` (the base
  `SmFilletSolver::PointOnPlaneSolve`). This replaces main's `as_assisted` views (`ConstantRadiusAssistedFs` clones of the base
  with invented radii `[0, 0]`, review of PR #249) for these calls without inventing data, and the virtuals' changes to the
  solver (offsets, surface extensions) stay in the table. Not modelled: a derived class in the table (upstream, an
  `SmConstantRadiusFS` there would dispatch its own `LoadInitialValues` etc.; the table would need a boxed `FilletSolverOps`
  per entry, a follow-up for whoever ports the executive's solver list). `sm_FindStepoffUV` (`blend.fillet_geom_p1`) still takes a
  `ConstantRadiusAssistedFs` and `CalcVertOnCrossSection` still passes it an `as_assisted` view (it reads base members only).
- **`sm_CreateCubicRailRailInterpolant`** takes two `&mut FilletSolver` (`SmFilletSolver*` upstream; main had
  `&ConstantRadiusAssistedFs`) and calls `FilletSolver::rail_rail_intersect`; the intersection points start as `SmTsectPnt()`, as
  upstream's locals. When both are the same table entry (one object upstream; the table cannot lend it twice) the second
  argument runs on a copy and its changes are dropped (`fillet_geom::p2::with_solver_pair`; also for `CalcRailIntRail`).
- **`CalcRailIntRail`** passes the vertex uses' points in and writes them back also on failure (they are in/out upstream).
  `CalcRailIntRail`, `CalcVertOnCrossSection` and `CalcSetBackVert` stop at the corner's filleted vertex
  (`blend.fillet_corner_p1`) before reaching the solver, so no compared case changes. The `blend.fillet_geom_p2` /
  `_p3` `calc_*` that reach a solver virtual take the table as `&mut [FilletSolver]`.
- **`SmFilletGeom::RailEdgeuseIntersect`** (`blend.fillet_geom_p6`) passes the vertex use's own `rTsectPnt` in/out (upstream's
  `CalcRailIntEdgeuse` hands it `pVU1->GetTsectPnt()`, which keeps what earlier guesses wrote; `CalcRailIntEdgeuse` writes it
  back also on a failure) and takes the target B-rep `&mut` (the solver may build the side edge use's UV trim curve). It sets
  the geometry for the surfaces, so the solver's `GetSurface` reaches `blend.fillet_solver_p1`'s justified
  `UNPORTED(blend.fillet_geom_p1): SmFilletGeom::GetOffsetSurface` ("blend.fillet_solver_p1: the geometry for the surfaces"):
  `blend.fillet_geom_p6`'s `rail_eu_intersect` cases with a solver now defer there instead of at this unit.
- **`ReCalcFilletGeom`** (`blend.fillet_geom_p3`) calls `CalcConeFilletGeom`, then `CalcTorusFilletGeom`, on copies of the
  sorted vertex uses' points (`SmTArray<SmTsectPnt*>` upstream; the builders only read them); a failing status of either is
  "not created", as upstream's `== SM_SUCCESS`, and `Unported` propagates.
- **`SmFilletIntersector`** runs `PointOnPlaneSolve` on `ConstantRadiusAssistedFs`, whose override is
  `SmConstantRadiusAssistedFS::PointOnPlaneSolve` (`blend.fillet_standard_solver_p2`): the intersector trait method has no
  default any more (the base version is never reached through it), and that implementation returns `Unported` for its owner.
