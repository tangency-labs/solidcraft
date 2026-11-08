# topology.brep_p10 edge/vertex tolerance last bits: SmSurfOfExtrusion::DropPointFast without NURB (2026-10-25)

Follow-up to the 2026-10-25 brep_p10 heal/notify fixes on main. `read_Fillet_bottle`,
`read_Fillet_cyl_cyl_shell`, `read_Fillet_fillet0227`, `read_Solids_InsetTaperedExtrude`
and `read_Solids_extrusion2` differed only in edge tolerances set by the healer's
`Fix_TolSizes` (`CalcEdgeZoneTol3d` of the edge's max edge/face gap) and the vertex
tolerances raised from them (rel 5e-12 to 2.6e-11). Instrumented both sides at
`Fix_TolSizes`, `SmEdgeuse::CalcMaxEdgeFaceGap`, `SmCrvSrfGapFunction::Evaluate` and
`SmSurface::DropPoint`: the gap function's drops onto `SmSurfOfExtrusion` faces had the
same inputs, but C++ `DropPointFast` succeeded (2 solutions; its `GetNaturalUVDomain`
makes the missing NURB lazily) while the port's returned `SM_ERR` because the surface,
read from file, had no NURB yet. The port then fell through to `LocalPointSolve`, whose
result is within an ulp or so of the analytic one, and the max gap length moved in the
last bits. `SurfOfExtrusion::drop_point_fast` now solves on an evaluation copy with the
NURB made (as the inherited queries already do via `base_with_nurb`) when the NURB is
missing. All five cases match bit for bit; entries removed from `known_mismatches.toml`.
