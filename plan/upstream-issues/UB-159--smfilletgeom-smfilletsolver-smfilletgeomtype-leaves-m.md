#### UB-159. `SmFilletGeom(SmFilletSolver*, SmFilletGeomType)` leaves `m_dOrientations` and `m_bCapped` uninitialised
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:8971-9004` (`SmFilletGeom::SmFilletGeom`).
- **What happens:** The constructor's initialiser list and loop set the type, solver, centre line, surface, split flag, status, offset surfaces, rails and deviations, but never `m_dOrientations[2]` or `m_bCapped[2]`. `m_bCapped` is assigned only by `SmFilletGeom::MakeFaceBrep` (`:9427`, `:9476`) and read by `SmFilletGeom::InsertIntersectionTopology` (`:12158`, `if (pV && !m_bCapped[ii])`) and `DumpLevel` (`:13306`): a geometry whose intersection topology is inserted before `MakeFaceBrep` has run reads indeterminate flags. `m_dOrientations` (`SmFilletGeom.h:546`) is never read or written after its declaration (the `m_dOrientations` of the solver classes are their own members), so it is merely dead and uninitialised.
- **Trigger:** `InsertIntersectionTopology` (or `DumpLevel`) on an `SmFilletGeom` before `MakeFaceBrep`.
- **How found:** read during porting (`blend.fillet_geom_p1`, PR #147).
- **Confirmed:** no (source inspection; the difftest never exports these members before they are set).
- **Severity:** UB (indeterminate reads).
- **Rust port:** `FilletGeom::new` starts them as `0.0` and `false`.
