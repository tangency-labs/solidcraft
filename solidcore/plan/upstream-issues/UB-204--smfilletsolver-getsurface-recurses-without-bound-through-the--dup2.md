#### UB-204. `SmFilletSolver::GetSurface` recurses without bound through the geometry set for the surfaces
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:136` (`SmFilletSolver::GetSurface`: `m_pFGForSurfaces->GetOffsetSurface(lRailIndex)`) and `source/SMLib/src/SmFilletGeom.cpp:9679` (`SmFilletGeom::GetOffsetSurface`: `m_pFilletSolver->GetSurface(lRailIndex)` when `m_pOffSurfs[lRailIndex]` is `NULL`).
- **What happens:** With a geometry set by `SetFilletGeomForSurfaces` whose own offset surface for the rail is `NULL` and whose `m_pFilletSolver` is the same solver, each function calls the other until the stack overflows.
- **Trigger:** `SetFilletGeomForSurfaces(pFG)` (`SmFilletGeom.cpp:11827`, `SmFilletCorner.cpp:7686-7693`, `:8092-8096`) on a geometry of the solver that never got its offset copies (`FinishConstruction` not run, or `SetOffsetSurface(i, NULL)` after it), then any `GetSurface(i)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** `FilletSolver::surface` returns `Unported` while a geometry is set for the surfaces (`plan/findings.md`, "blend.fillet_solver_p1: the geometry for the surfaces"); no Rust caller sets one yet.
