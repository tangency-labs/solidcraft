# blend.fillet_solver_p5: a closed edge's rail past its own corner was taken for a cliff overflow (2026-11-03)

Updates: plan/findings/blend.fillet_solver_p5--fork-rails-fit-their-faces.md

- **Unit:** `blend.fillet_solver_p5` (`FilletSolver::rails_fit_their_faces_judged_with_ops`,
  `crates/solidcore/src/blend/fillet_solver/p5/rails_fit.rs`), only with
  `Kernel::fixes.fillet_refuse_rails_leaving_faces` (and, for what follows from it,
  `fillet_cliff_overflow` and `fillet_overflow_trim_at_creases`).
- **Defect (tangency-labs/solidcore#2498):** a closed filleted edge that meets itself at an
  angle (a teardrop prism's rim) has one vertex for both ends, and the face it bounds alone
  (the prism's top) has no side edge there. The rail of one end's blend, untrimmed, runs on
  past the rails' crossing (where the corner cuts it) and crosses the rim's other stretch
  r / sin a from the tip, a the corner's interior angle. `LeavesPastAnEnd` asks for a side
  edge at an end, or for the edge point nearest where the rail leaves to be an end; here
  the nearest edge point is the crossing itself, on the edge's other stretch. So the run
  was taken for a rail leaving its face beside the edge, the blend for a cliff overflow,
  and `TrimAtCreases` then imprinted the side face's rail, which runs from the seam round
  to the seam, without splitting the face. That left an edge with no edge use, and the
  next step failed with "edge without primary edge use". The fork (tangency-labs/smlib
  v1.1.0-alpha, `SmFilletSolver::LeavesPastAnEnd`) has the same test, so the same flaw.
- **Input:** the teardrop of `crates/solidcore/tests/wr697_end_and_join.rs` (one closed
  cubic B-spline, poles (0,0) (8,-8) (16,-4) (16,4) (8,8) (0,0), knots 0, 1/3, 2/3, 1)
  swept 10 up z; its top rim rounded r 0.5 or r 1 with every fix but
  `geometric_g2_continuity`.
- **Values:** before: `do_fillet` fails ("edge without primary edge use"). After: a clean
  body of five faces (the round in two halves, mitred at the tip by the closed Nx1 corner),
  volumes 1217.172410518837 (r 0.5) and 1210.854023851367 (r 1), 7.7e-7 and 1.9e-7 of
  themselves under the closed form of a round mitred at the tip
  (10 A - L r^2 (1 - pi/4) + (3 pi/4 + 1) r^3 (5/3 - pi/2)).
- **Change (inside the flag only):** a first or last off-face run is also past an end when
  the filleted edge is closed and the run lies beyond the plane on which the corner at its
  vertex cuts the rail: the plane through the vertex of normal T0 + T1 (the edge's unit
  tangents at its start and end; the cross section there for a G1 closure). The rail where
  it comes onto the face is still to cross that plane ahead of it (or, where it leaves the
  face, has crossed it behind it). A run that leaves the face on its own blend's side of
  the plane is judged as before, so a closed rim too wide for its face is still an
  overflow. Open edges are unchanged.
- **Where the corner comes from:** the corner at the tip is still `SmFilletNx1ClosedCorner`
  (the fork's too, at v1.1.0-alpha); the closed edge's own corner event is #2349. This change
  is in the overflow judgement, which that event keeps.
- **Test:** `a_closed_convex_rim_meeting_itself_at_an_angle_is_rounded` in
  `crates/solidcore/tests/wr697_end_and_join.rs` (fails without the change).
