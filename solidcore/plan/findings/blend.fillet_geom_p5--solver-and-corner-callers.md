# blend.fillet_geom_p5: solver and corner callers

Five `UNPORTED(blend.fillet_geom_p5)` stubs stay in units merged before this one:
`SmFilletGeom::SmFilletGeom` (copy), `CreateEndCurve` and `TrimRailCurves` in
`FilletSolver::split_fillet_geom` / `trim_rails`, `SmFilletEdge::MakeFilletEdge` in
`sm_SplitRail` (`blend/fillet_solver/p1.rs`, `blend.fillet_solver_p1`) and in the corner
operations of `blend.fillet_corner_p3` (`blend/fillet_corner/p3.rs`). Those callers hold a
fillet geometry as a data-less `FilletGeomRef { index }` and the pseudo-B-rep as a bare
`topology::Brep`, while the ported routines are methods of `FilletBrep` (the pseudo-B-rep
with its fillet side tables, `blend.fillet_geom_p1`) addressed by `FilletGeomId` / vertex
ids and need the solver table (`&[FilletSolver]`). There is no map from a `FilletGeomRef`
to a `FilletGeomId` and no `FilletBrep` in those callers, so the calls are marked
`justified:` with this section; each still returns `Unported`. The same callers' other
fillet-geometry calls (`SmFilletGeom::GetRail`, `SetUpRailPoint`, `SmFilletVertex`
constructor, `blend.fillet_geom_p1` / `_p6`) are stubs for the same reason; wire them all
together when the solver and corner units address geometries in a `FilletBrep`. The
targets are `FilletBrep::copy_fillet_geom`, `create_end_curve`, `trim_rail_curves` and
`make_fillet_edge` (`blend/fillet_geom/p5.rs`). No disagreement.
