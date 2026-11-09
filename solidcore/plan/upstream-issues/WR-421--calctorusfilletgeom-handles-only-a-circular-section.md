#### WR-421. `CalcTorusFilletGeom` revolves only a circular section, so a chamfer on an arc is traced as an offset intersection that collapses to a point
- **Severity:** Wrong result
- **Area:** Blends
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::CalcTorusFilletGeom`: `if (m_pFSG->GetFilletSurfaceGeneratorType() != SM_FSG_CIRCULAR || !pCurve->IsArc(...)) return SM_ERR;`.
- **What happens:** a linear section (a chamfer) on a circular edge between surfaces of revolution about the arc's axis is not made as the revolved section, but traced as the intersection of the offset surfaces. Where the neighbouring round's offset reaches its axis (a chamfer as wide as the corner round it runs round, or the rim of a through hole under `SmApiChamferFillet`), that intersection is a single point and the blend fails with `SM_ERR`.
- **Trigger:** a 40 x 40 x 20 plate with its vertical edges rounded R1, then its bottom loop chamfered 1 (tangency-labs/solidcore#420); a cube with a through hole under `SmApiChamferFillet` (#379, 1006).
- **How found:** SolidCraft's rig dump 2719 (#420), a cube with a through hole (#379).
- **Confirmed:** yes (reported with the fix and the capi tests in tangency-labs/smlib@8a7deae, @f9299b7).
- **Fix:** tangency-labs/smlib@8a7deae revolves a linear section too, from the line between the rail points, giving a cone.
- **Rust port:** `torus_fillet_geom_parts` (`crates/solidcore/src/blend/fillet_solver/p2.rs`) refuses a linear section as upstream by default. With `Kernel::fixes.fillet_chamfer_cone_on_arc` it revolves the line between the rail points. Tests `issue_420_chamfer_on_arc`: the plate (fails with the fix off, volume equals the closed form with it on) and the rims of the through hole (removed only with the fix).
