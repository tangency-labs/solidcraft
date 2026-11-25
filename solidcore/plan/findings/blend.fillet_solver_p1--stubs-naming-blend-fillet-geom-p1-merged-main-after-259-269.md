# blend.fillet_solver_p1: stubs naming blend.fillet_geom_p1 (merged main after #259/#269)

`blend.fillet_geom_p1` is `partial` on main, so the audit lists this unit's 48 `UNPORTED(blend.fillet_geom_p1)` sites as CHECK
(`SmFilletGeom::Get/SetOffsetSurface`, `GetRail`, `SetFilletGeomType`, `SmFilletEdge::Get/SetOriginalFace`, `SmFilletVertex`
accessors, `SmFilletEdgeuse::SetUVCurve/Get/SetFilletGeom`, ...). They are not wired here: the ported `FilletGeom` / `FilletEdge` /
`FilletVertex` live inside a `FilletBrep` and are addressed by `FilletGeomId` / `EdgeId` / `VertexId`, and a `FilletGeom` stores a
`FilletSolverId`, while this unit's solver owns opaque `FilletGeomRef` handles and its `FilletSolverOps` methods take `&Brep` (not
`&mut FilletBrep`). Wiring means replacing the handles by `FilletBrep` ids and threading `&mut FilletBrep` through `FilletSolverOps`,
`blend.fillet_custom_solver`, `blend.fillet_intersector` and the three shims (the 7 deferred `FinishConstruction` cases of this unit's
report, which stop at `SetOffsetSurface`, are the first to run). That signature change touches every downstream solver PR and is left
to a follow-up. The two sites that were wireable are wired: `SmFilletSolver::FindIndexOfRailOnFace` (`blend/fillet_geom/p1.rs`,
`sm_CreateCubicRailRailInterpolant`) and the `SmCircularCrossSectionFSG` constructor (`offset/offset_geometry_creation.rs`).
