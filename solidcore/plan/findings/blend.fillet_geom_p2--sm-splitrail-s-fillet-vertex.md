# blend.fillet_geom_p2: sm_SplitRail's fillet vertex

`sm_SplitRail` (`blend.fillet_solver_p1`) builds `SmFilletVertex(pFG->..->GetFilletCorner())`
for the vertex it splits the rail at. The constructor is ported (`FilletVertex::new` with
`FilletBrep::insert_fillet_vertex`, `blend.fillet_geom_p2`, unit `blend.fillet_geom_p1`
owns the class), but `split_rail` works on the bare pseudo `Brep` and has no `FilletBrep`
to insert into. The call site stays an `UNPORTED(blend.fillet_geom_p1)` stub (tag
retargeted from the stale `blend.fillet_geom_p2`, which does not own the symbol); it
is wired together with the other `blend.fillet_geom_p1` stubs once `&mut FilletBrep` is
threaded through the solver (see "blend.fillet_solver_p1: stubs naming
blend.fillet_geom_p1"). The `split_rail` body is unchanged.
