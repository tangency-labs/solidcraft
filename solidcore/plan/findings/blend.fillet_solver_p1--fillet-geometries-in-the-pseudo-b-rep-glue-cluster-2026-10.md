# blend.fillet_solver_p1: fillet geometries in the pseudo B-rep (glue cluster, 2026-10-20)

Follow-up to the section above. The solver's `FilletGeomRef` stand-in is gone: `FilletSolver::fillet_geoms` /
`fg_for_surfaces` hold `blend.fillet_geom_p1`'s `FilletGeomId`s, the geometries live in the executive's pseudo B-rep
(`FilletBrep`), and the functions that reach them take it (`pseudo: &mut FilletBrep`). Wired (35 sites):
`FinishConstruction` (`SetOffsetSurface`; the 7 deferred `finish` cases now run), `SplitFilletGeomAtSideEdge` (all
`SmFilletGeom` / `SmFilletEdge` / `SmFilletVertex` accessors, plus its `SmVertex::SetPoint` on the pseudo B-rep through
`FilletBrep::set_vertex_point`), `SplitFilletGeom` (`GetRail`, `SmFilletVertex::SetStatus`) and the four geometry writes of
`SmFilletIntersector::FlushCurve` (`SetFilletSurface`, `m_pCenterLineCurve`, the rails' `SetCurve` + `SetInterval`, the mate
edge uses' `SetUVCurve`; the trait methods now take the pseudo B-rep, `flush_curve` passes it). No compared value changes
besides the 7 `finish` cases: `SplitFilletGeomAtSideEdge` stops at its first call (`GetRailIndexOfVertexuse`,
`blend.fillet_geom_p6`), `SplitFilletGeom` at `GetRailVertex` (`_p6`) and `FlushCurve` at the executive before the writes.

- **Constructor.** `SmFilletSolver()` runs `m_vFilletGeoms.Add(new SmFilletGeom(this))`; the geometry needs the pseudo B-rep
  and the solver's identity (`FilletSolverId`). `FilletSolver::new_in(k, pseudo, id, ..)` is the full constructor;
  `FilletSolver::new` (still called by the derived solvers' constructors of `blend.fillet_standard_solver_p1` /
  `blend.fillet_custom_solver`, whose porters thread the pseudo B-rep later) has no geometry, and
  `FilletSolver::add_constructor_fillet_geom` adds it. A solver made by `new` alone returns `InvalidInput` from
  `GetFirst/LastFilletGeom`, where upstream returns the constructor's geometry; no compared case reads it (the intersector and
  solver runners now add it, as the shims' solvers have it).
- **`FilletGeom::new` / `offset_surface`** take the `FilletSolver` base instead of `ConstantRadiusAssistedFs` (all callers passed
  `None`). `GetOffsetSurface`'s fallback `m_pFilletSolver->GetSurface` is the calling solver (the geometries are its own).
