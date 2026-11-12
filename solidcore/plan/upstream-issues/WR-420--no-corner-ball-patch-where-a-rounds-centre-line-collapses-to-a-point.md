#### WR-420. A round between two cylinders as wide as the round has no analytic geometry (`CalcSphereFilletGeom` is missing), so its offset intersection collapses to a point and the blend fails
- **Severity:** Wrong result
- **Area:** Blends
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, the analytic attempts after `CalcCylinderFilletGeom` (`CalcTorusFilletGeom` is the last, `:2000`); the rest is traced as the intersection of the offset surfaces.
- **What happens:** where two bottom rounds of radius R meet at a corner, the seam between them is the edge a following round of radius R runs down. Each cylinder's offset by R is its axis, so the offsets intersect in the single point where the axes cross, every ball touching both is centred there, and the intersection trace is one point, not a curve. The round fails with `SM_ERR` (SolidCraft sees status 1001).
- **Trigger:** a 25 x 25 x 10 box whose bottom edges are rounded R2, then its 4 vertical edges R2 (each one's tangent chain runs down the seam of two bottom rounds); tangency-labs/solidcore#416 (SolidCraft rig case fillets/1557).
- **How found:** SolidCraft's rig, case fillets/1557.
- **Confirmed:** yes (reported with the fix and the capi test, which matches the closed form, in tangency-labs/smlib@04bd9a5).
- **Fix:** tangency-labs/smlib@04bd9a5 adds `CalcSphereFilletGeom`, which builds the corner ball's patch between the two contact circles.
- **Rust port:** off by default (`Kernel::fixes.fillet_corner_ball_patch`). With it, `FilletSolver::calc_sphere_fillet_geom` (`crates/solidcore/src/blend/fillet_solver/p2.rs`) is tried after the torus. The patch is the arc of the first contact circle turned about the line square to both axes onto the second circle. Test `issue_416_fillet_seam`: fails with 1001 by default; with the flag the removed volume equals the closed form to 1e-7 relative.
