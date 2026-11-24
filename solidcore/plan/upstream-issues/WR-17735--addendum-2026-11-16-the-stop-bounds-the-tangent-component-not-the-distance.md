#### WR-17735 addendum (2026-11-16). The intersection's stop bounds the gap along the tangent, not the distance

- **Correction:** WR-17735 says that, for the minimisation that decides an intersection,
  dividing `F` and `F'` by `|C'|` makes the `|F| < SM_EFF_ZERO` stop leave "the foot at
  most `SM_EFF_ZERO` along the curve from the point". That residual stop bounds the gap's
  component along the unit tangent at the foot, not the distance from the point; the
  solve's other stops, unchanged (a gap below `SM_EFF_ZERO`, or one within an angle of
  `SM_EFF_ZERO` of square to the tangent), bound that component only relative to the gap. A stationary point of
  the distance that is not the point stops the iteration too, as it did before the change:
  on `C(t) = (0.1 t (t - 1), 0.1 (1 - (1 - t)^3))` with `P = C(0)`, the end `t = 1` is
  0.1 from `P` and its gap is square to the tangent.
- **Why that is no wrong answer:** the stop only ends the Newton iteration.
  `Curve::local_point_solve_held` returns the foot's distance `|C - P|` as the solution's
  value, and `SmCPGlobalSolver::LocalSolve` (`CpGlobalSolver::local_solve_branch` in
  `crates/solidcore/src/curve/curve/p1.rs`) keeps an intersection's foot only when that
  distance is within the tolerance. The code comments of `FindPcExtremaEfo::evaluate`,
  `FindPcExtremaEfo::intersecting` and `Curve::local_point_solve_held` now say so.
- **Test:** `an_intersection_is_decided_by_the_distance_not_by_where_the_search_stops`
  (`crates/solidcore/src/curve/curve/p3.rs`): on the curve above, the search from `t = 1`
  stops there with its real distance 0.1, and the intersection with `P` within 1e-9 returns
  only `t = 0`.
