#### WR-552. A revolved arc that reaches the axis within the tolerance sweeps a spindle or ring torus, not a horn one
- **Severity:** Invalid result (a face loop turned over, zero area)
- **Area:** Sweeps (rotational sweep, analytic faces)
- **Location:** the SolidCraft fork's `SmRotationalSweepGeometry::AnalyticSurface`, the torus branch. This is fork code (tangency-labs/smlib 0b143f7, 9853e54), ported as `sweep/analytic_revolution.rs` behind `revolve_analytic_profile_faces` (WR-448). Pinned SMLib sweeps a B-spline surface of revolution there, which has the same geometry.
- **What happens:** the rotational sweep takes a vertex within its tolerance of the axis to be on it (`sm_CheckEdgeVerticesOnAxis`), so the swept vertex is a point. The arc itself is swept as it lies. If it is a hair inside the axis, the result is a spindle torus; if a hair outside, a ring torus. Either way the minor circles cross or miss the axis where the topology collapses, and the face's trims there come out with the loop turned over (zero area).
- **Trigger:** a profile arc whose circle passes within the tolerance of the axis, ending there. The fork's case is "the tight bend", whose arc is 2.3e-10 tighter as SolidCraft's sketch solves the reference case's three-point arc.
- **How found:** SolidCraft. The fork's fix is tangency-labs/smlib@c4e378d ("a circle reaching the axis within the tolerance sweeps a horn torus"), which is not merged upstream.
- **Confirmed:** no C++ run was made here.
- **Rust port:** behaves as before by default. With `Kernel::fixes.revolve_arc_touching_axis_is_horn_torus`, the torus's major radius is its minor one when they differ by at most the caller's tolerance (the sweep's `dEps`, as in the fork). The port then runs its usual exact layout test against `sm_CreateRotSrf`'s B-spline layout (`min(tol, 1e-9 (1 + size))`).
  - The snap moves the surface by at most the difference, so the horn torus is kept for differences within that fit tolerance, such as the fork's 2.3e-10 case.
  - A larger difference, up to the sweep tolerance, fails the layout test, and the B-spline surface of revolution stays, as before. That is a narrower reach than the fork's, which has no layout test.
  - The test exists because the faces' trims and edges come from the B-spline layout and must lie on the surface (#1463, #1490, WR-471). A design that put the horn torus through both arc ends instead was rejected: a horn circle through an end on the axis is tangent to the axis there, which moves its centre by about `sqrt(2 R delta)`, far more than the tolerance.

  Test: `tests/wr_552_revolve_arc_touching_axis.rs`.
