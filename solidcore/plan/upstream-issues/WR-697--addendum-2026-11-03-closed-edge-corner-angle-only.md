Updates: WR-697

#### WR-697 (update, 2026-11-03). The closed-edge corner re-tests its edge for G1, after `Create` has made every G1 one the Join, and keeps a tangent solver, a tangent adjustment and cross-section clauses that only that case reaches
- **Severity:** Wrong result
- **Area:** Fillets (corners)
- **Location:**
  - `source/SMLib/src/SmFilletCorner.cpp`, `SmFilletNx1ClosedCorner::MakeCornerTopology`: its `sm_TestG1Continuity` of the filleted edge with itself and the G1 branches it guards (a `MATE` vertex, a `CROSS_SECTION` edge, `m_bBlending`); `SmFilletNx1ClosedCorner::GetTangentFilletSolver`; `SmFilletNx1ClosedCorner::AdjustTangentCorner`.
  - `source/SMLib/src/SmFilletGeom.cpp`, `SmFilletEdge::CalcCrossSection`: the `SM_FCR_NX1_CLOSED` clauses (the connectivity check of a blending closed corner, the two UV curves of a closed tangent surface).
- **What happens:**
  - With the blend corner's step 4d (the fork's 543d25a), `SmFilletCorner::Create` makes a closed blended edge with one to three edge uses that meets itself G1 (`sm_TestG1Continuity` at the solvers' tangency tolerance) the blend corner's Join. Only an edge meeting itself at an angle stays the closed Nx1 corner.
  - That corner's topology runs the same test on the same edge, vertex and tolerance, so its G1 branches never run and `m_bG1Case` is always false. Its tangent solver, which answers only for a G1 edge, is always `NULL`.
  - Its tangent adjustment acts only where the edge's two end tangents are parallel within 2 degrees, which `Create` has already made a Join at any tangency tolerance of 2 degrees or more (10 by default, 2 in `SmApiFillets`): at most at the rounding of an angle of exactly 2 degrees.
  - `CalcCrossSection` runs on a corner's `CROSS_SECTION` edges. The closed corner now makes none and is never blending, so its clauses there never hold. The closed Join keeps them.
- **How found:** the SolidCraft fork's smlib#113 (tangency-labs/smlib caa67d0 and 2ac7a17, merged at 2ac7a17). The fork deletes these branches: the closed corner keeps only its angle case, and its tangent solver and adjustment are the base class's (`NULL`, success).
- **Rust port:** under the same opt-in `Kernel::fixes.fillet_blend_corner`, off by default.
  - `make_nx1_closed_corner_topology` doesn't test the edge for G1 and leaves `g1_case` false.
  - `tangent_fillet_solver_nx1_closed_with_lookup` returns `None`, and `adjust_tangent_corner_nx1_closed` does nothing.
  - `calc_cross_section`'s `NByOneClosed` blending clause and the `NByOneClosed` terms of its two closed-surface tests don't apply; the closed Join's do.
  - The pinned default is unchanged and still reaches all of this code, so nothing is deleted yet. It goes when the flag becomes the default (AGENTS.md section 4b); the list is in `plan/findings/blend.fillet_corner_p3--fork-smlib-113-old-corner-code-still-reached--addendum-2026-11-02-closed-edge-corner.md`.
- **Confirmed:** in the port (PR #2436). `a_closed_edge_meeting_itself_at_an_angle_is_the_closed_corner` in `crates/solidcore/tests/wr697_end_and_join.rs` rounds the foot of a teardrop boss on a plate, one closed edge meeting itself at a right angle, at r 0.5 and r 1, with and without the flag: the closed Nx1 corner in each case, 10 faces, the body clean, and every volume the same to the bit before and after the change. The unit test `a_closed_g1_corner_s_tangent_solver_is_its_own` in `blend::fillet_corner::p2` checks the tangent solver at a cylinder's rim: the corner's own solver without the flag, `None` with it. Every integration test that turns the flag on prints the same output before and after.
