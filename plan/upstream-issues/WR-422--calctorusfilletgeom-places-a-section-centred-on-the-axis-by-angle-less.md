#### WR-422. `CalcTorusFilletGeom` places a section centred on the arc's axis by rail points that carry no angle, so the torus can sweep the wrong way
- **Severity:** Wrong result
- **Area:** Blends
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::CalcTorusFilletGeom`: the rail points `sP0`/`sP1` and `sEndP0`/`sEndP1`, and the sweep direction taken from the edge point nearest `sArcCenter` (`GlobalPointSolve`).
- **What happens:** a round as wide as the corner round it runs round is centred on that round's axis, where the offset of a face collapses. The rail points of the intersection points then lie anywhere on the circle the ball touches, and every point of the edge is as near the centre, so the torus can sweep 270 degrees the wrong way and the rollover test fails (`SM_ERR`).
- **Trigger:** a 297 x 100 x 60 block with its vertical edges rounded R5, then its top rim rounded R5 (tangency-labs/solidcore#417); on a smaller box it happened to turn the right way.
- **How found:** SolidCraft rig, 297 x 100 x 60 (#417).
- **Confirmed:** yes (reported with the fix and the capi test in tangency-labs/smlib@110b131).
- **Fix:** tangency-labs/smlib@110b131 turns the rail points into the half-planes through the edge's ends (`sm_TurnIntoHalfPlane`) when the centre is on the axis, and takes the edge's start for the nearest point.
- **Rust port:** `torus_fillet_geom_parts` (`crates/solidcore/src/blend/fillet_solver/p2.rs`) behaves as upstream by default. With `Kernel::fixes.fillet_torus_center_on_axis` it turns the rail points as the fork does. Test `issue_417_torus_center_on_axis`: fails with the fix off, volume equals the closed form with it on.
