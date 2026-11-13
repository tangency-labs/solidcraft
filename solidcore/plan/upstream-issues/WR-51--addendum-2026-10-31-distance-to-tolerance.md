Updates: WR-51

#### WR-51 (update, 2026-10-31). The start-point test compares a real distance to its tolerance behind an opt-in flag

- **What changed:** `SmSurfaceIntersector::IsPointOnStartPoint` tests each surface point
  with `DistanceBetweenSquared(...) < m_dThisApproxTol3d`, i.e. a squared distance
  against a length, so the effective acceptance radius is `sqrt(m_dThisApproxTol3d)`
  (`1e-3` for `tol = 1e-6`, and tighter than intended for `tol > 1`). With the flag the
  squared distance is compared with the squared tolerance, so a point is accepted only
  within a real distance of `m_dThisApproxTol3d`.
- **Trigger:** a candidate point whose distance from a start point lies between `tol`
  and `sqrt(tol)`: the pinned test accepts it and the fixed test refuses it. In the Rust
  unit test
  (`intersect::surface_intersector::p1::tests::is_point_on_start_point_accepts_a_real_distance`)
  a `0.1`-sized plane gives `this_approx_tol_3d = 1.414e-4`; a point `1e-3` from the
  start point (squared distance `1e-6`) is accepted pinned and refused with the flag.
- **Confirmed:** the pinned source and the difftest record: `edge_start_points` and
  the generated `on_start` ops agree with the squared comparison. The C++-side native
  pair for the opt-in path was not measured in this lane; the Rust-only pair above
  records the pinned and corrected results.
- **Opt-in:** `Kernel::fixes.point_on_start_point_squared_tolerance`, default `false`
  (pinned C++ parity). `SurfaceIntersector::is_point_on_start_point` takes the kernel
  and, when set, compares each surface point's squared distance with
  `this_approx_tol_3d * this_approx_tol_3d`; both the first and second surface
  comparisons are gated. Its two callers pass the kernel. Off by default the pinned
  expression is unchanged.
- **Rust port:** still reproduces the pinned result by default; the difftest default
  path is unchanged.

addendum_options: Kernel::fixes.point_on_start_point_squared_tolerance
