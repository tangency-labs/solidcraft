#### WR-572. A chamfer round an arc is stored as a surface of revolution, not the cone it is
- **Severity:** Wrong result (surface type)
- **Area:** Blend (fillet solver)
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::CalcTorusFilletGeom`: `pFilletGeom->m_pFilletSurface = pTorus;` for every cross-section. The function is reached with a linear section only with the fork's earlier change (WR-421, #420); the pinned 1.0.1 build refuses one there.
- **What happens:** `CalcTorusFilletGeom` revolves the fillet's cross-section about the arc's axis. A circular section gives a torus and a linear one (a chamfer) a cone, but both are kept as an `SmSurfOfRevolution`. A straight edge's round is stored as its cylinder (`CopyAndAddAnalytics` in `CalcConeFilletGeom`), whereas the chamfer round a hole's rim stays a revolution. A reader that takes a revolution back as a rational B-spline (SolidCraft) then has no cone for that face.
- **Trigger:** a chamfer along a circular edge, e.g. the rim of a through hole in a box, with the linear section revolved (WR-421).
- **How found:** SolidCraft (fuzz `a_patterned_bore_preserves_the_analytic_surfaces` counted the rim as "other"). The fork's fix is tangency-labs/smlib@b5743ec, which is not merged upstream: a linear section's revolution is copied to its NURBS form and passed through `CopyAndAddAnalytics`, and the `SmCone` it gives (same parameters) is stored, the rails made on it; anything else stays the revolution.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`) plus WR-421.
- **Rust port:** with `Kernel::fixes.fillet_chamfer_cone_on_arc` alone the chamfer is a surface of revolution, as in the fork before b5743ec. With `Kernel::fixes.fillet_chamfer_cone_stored_as_cone` as well, it is stored as the cone (`blend/fillet_solver/p2.rs`, `torus_fillet_geom_parts`).

  Tests: `crates/solidcore/tests/wr_572_chamfer_rim_stored_as_cone.rs` (#379's 10 cube with a radius 2 through hole, every edge chamfered with legs 0.5: two cone faces and no revolution with the fix, two revolutions without it; the volume removed matches the cube's and the rims' chamfers to 1e-6).
