#### WR-568. `CalcTorusFilletGeom` takes the arc's own start angle off the round's span
- **Severity:** Failure (status 1001 on a fillet)
- **Area:** Blend (fillet solver)
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::CalcTorusFilletGeom`: `dEndAng = SM_RAD2DEG(dSpanAng); double dAngleSpan = dEndAng - dStartAng;` (`:1244` at 1.0.1).
- **What happens:** the round of a circular edge is a surface of revolution whose span is the turn from the start section to the end one (`dSpanAng`, `CCWAngleBetween` about the axis). The code then takes `IsArc`'s start angle off that turn. `SmCurve::IsArc` gives 0 there, but `SmCircle::IsArc` gives the start of the circle's analytic domain, measured from the circle's own frame; a boolean's circle framed on a cylinder's seam starts at 60 or 115 degrees. The torus comes out short by that angle, and the end rib (the round crossing the face the edge ends in) finds no fillet surface at the far end: `sm_TwoPntsIntersection` drops 7.5 mm off and the fillet fails with 1001 ("Corner Edge Computation Failure" at an Nx1 corner).
- **Trigger:** rounding an arc edge whose curve is an `SmCircle` with an analytic domain that does not start at 0, e.g. the edge a cylinder cut leaves on a box face, with the cylinder's seam away from the arc's start.
- **How found:** SolidCraft (`kernel::blend::tests::a_curved_edge_blend_ends_in_a_face_at_an_angle`, `kernel::issue_851_tests::fillet_the_three_edges_at_the_arcs_front_corner`). The fork's fix is tangency-labs/smlib@985faae (smlib#64), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.fillet_torus_span_from_start_section`, the span is the turn alone (`blend/fillet_solver/p2.rs`).

  Tests: `crates/solidcore/tests/wr_568_round_along_an_arc_span.rs` (the fork's capi case `groove_arc_round_ends_in_the_top`: the 40 x 20 x 20 box less a cylinder r 8 along y, 4 below the top, seam turned up; the 240 degree front arc rounded r 0.5).
