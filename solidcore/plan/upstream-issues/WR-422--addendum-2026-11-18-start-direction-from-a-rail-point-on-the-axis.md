Updates: WR-422

#### WR-422 (update, 2026-11-18). The start section's turn is taken from a rail point on the axis

Still in v1.0.1 (3fd5b95): `SmFilletSolver::CalcTorusFilletGeom` (`SmFilletSolver.cpp:932`-`:937`)
takes the direction its section starts to turn from as `sP0 - sOrigin`, and falls back to
`sP1 - sOrigin` only when that vector is shorter than `dAnalyticTol`. The end section
(`:1000`-`:1004`) falls back when the vector's cross product with the axis is that short, that
is when the rail point lies on the axis anywhere, not only at the origin.

- **What happens:** where a face's offset collapses onto the axis (a cylinder whose radius is
  the ball's, beside a plane square to its axis) every ball centre lies on the axis and the
  plane's rail point is the ball's pole, on the axis a radius above the origin. Taken as the
  start direction, it is parallel to the axis: the sweep direction `sAxisOfRevolution * sVec`
  is zero, so the axis's sense is not decided by the edge's tangent, and the turn to the end
  section is measured from a vector with no direction about the axis. Whether this is hit
  depends on which face is the solver's first: with the cylinder first, `sP0` is its contact
  point off the axis and the turn is right.
- **Trigger:** #17594, a boss's six-edge rim rounded at the radius of the rounds on its upright
  corners. At the one corner whose round had the plane as its first face, the section to the
  next straight round came out up to 0.61 off its meridian and 0.21 off that round's cylinder:
  the full check reports `Bad Gap` (0.206419 against 0.020039), and the round's volume is off
  where its faces miss each other.
- **Change:** the start section falls back as the end one does, and both judge a rail point on
  the axis within the solver's tolerance (`approx_tol_3d`, which WR-592 already uses to snap a
  rail point onto the axis), not within the absolute `dAnalyticTol`, which a point's rounding
  far from the origin exceeds.
- **Test:** `solidcore-ops` `blend::a_hex_rim_rounded_at_its_corner_rounds_radius_closes_on_six_spheres`:
  the slab of #17594's boss, its rim rounded r 1, is valid, has the counts [26, 54, 30], six
  corner pieces each on the unit ball about its corner's axis, and removes the closed form
  `6 ((1 - pi/4) Rc + pi/18)` within 1e-9 relative. It fails on `main` at the check.
