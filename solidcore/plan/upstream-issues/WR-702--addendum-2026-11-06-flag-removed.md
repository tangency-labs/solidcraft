Updates: WR-702

#### WR-702 (update, 2026-11-06). `face_shrink_bound_on_knot` is removed

The fix is the default (tangency-labs/solidcore#2273, step 4a): `topology::face::p1::shrink_geometry`
always takes a bound within `64 * DBL_EPSILON` times the knot vector's largest |knot| of one of a
B-spline surface's knots as that knot before the trim. The fork scales by the knot's own magnitude,
which leaves a knot at 0 no window. The flag field and its entry in the rig's `fixes.json` are gone.

- **Tests:** `bound_a_rounding_step_past_a_knot_ends_on_the_knot` runs on `Kernel::default()` (1
  span along u); its flag-off half (2 spans) is removed.
  `bound_a_rounding_step_past_a_knot_at_zero_ends_on_the_knot` ends a bound a rounding past a
  knot at 0 on it; it fails with the fork's scale.
- **Results:** the G^n rounds of `tests/wr685_gn_equal_area_per_station.rs` and
  `tests/wr698_gn_round_is_its_definition.rs` (the chamfered box's two edges, the pipe on the
  plate) are one span along their edge, ending on it. The fork's nets those tests compared keep a
  sliver span past the edge's end (7 rows in u, the faces ending 6.1e-4 past the chamfered box's
  40 and 9.1e-4 past the pipe's 60). The tests compare the fork's sections and take x from the
  edge's length; the volumes are unchanged within the integrator's 1e-7. The rig ran with the
  flag on and the fork's scale; the kernel rig at the head is the PR's full gate.
