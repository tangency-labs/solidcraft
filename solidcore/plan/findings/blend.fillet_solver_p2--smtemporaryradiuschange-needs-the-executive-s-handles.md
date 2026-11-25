# blend.fillet_solver_p2: SmTemporaryRadiusChange needs the executive's handles

`blend.fillet_executive_p2` (partial on main since 2026-10-21) ports `SmTemporaryRadiusChange` as
`fillet_executive::p2::TemporaryRadiusChange<O>`, over the executive's opaque handles: its constructor and `SetUp` take
`&mut impl ExecutiveAccess<O>` and the solver and side edge use as handles `O` (`variable_radius_solver(fs)`,
`solver_edgeuse(fs, i)`, `edgeuse_edge`, `edge_vertices`). `SmFilletSolver::RailRailIntersect` here runs on
`&mut dyn FilletSolverOps` and two typed `Brep` ids, with no `ExecutiveAccess` adapter and no handle for either solver, and
`FilletSolverOps` has no cast to `VariableRadiusFs`. So the two `SmTemporaryRadiusChange` locals stay `UNPORTED` stubs behind the
`IsKindOf(SmVariableRadiusFS_TYPE)` class test (`SetUp` returns at once for every other class, which is what the class test
reproduces), until the executive owns the solvers and can hand `RailRailIntersect` its access and handles. No compared case reaches
them: the difftest's test solver is not an `SmVariableRadiusFS`.
