Updates: WR-647

#### WR-647 (addendum 2026-11-24): `simple_coincidence_crossing_is_point` is retired to upstream's range

- **Removed:** the `UpstreamFixes::simple_coincidence_crossing_is_point` field and its doc
  (`src/kernel/fixes/simple_coincidence_crossing_is_point.md`), the fork's "case 3e" block in
  `Curve::simple_coincidence_checker` (`curve/curve/p6.rs`) and the error it alone raised,
  `CurveFailure::NearestNotFound`. A one-sided stretch within tolerance is now always the
  coincidence range upstream returns. Crossings and touches seen through the tolerance stay
  decided on the merged range in `GlobalCurveIntersect` (addendum 2026-11-18).
- **What replaces it:** the face/face section tangent to a floor arc at their shared end vertex
  (#18064) is made one point by the tangency witness (`intersect/curve_class/witness.rs`), from
  the surfaces. Its flag gate is removed, so the witness always runs on a range result of an
  unseamed edge. `approaches_without_crossing` (p6.rs) stays: the witness uses it.
  `drop_ranges_of_tangencies` (p8.rs) stays: it never read the flag; it drops the flank
  ranges beside a touch.
- **Why the range is right:** two concentric arcs `gap <= tol` apart are `gap` apart at every
  angle, so every point is a nearest point and no single one is distinguished. The range is
  the closed-form answer (`wr_647_concentric_arcs_within_tolerance_are_a_range`). Case 3e's
  nearest point turned an edge against a fitted copy of itself into points (#1036).
- **Tests:** tests that ran only with the flag on, or repeated a flag-off test, are deleted:
  - `wr_647_concentric_arcs_within_tolerance_are_their_nearest_point` (the flag's own
    expectation, superseded by the closed-form range test);
  - `wr_647_crossing_is_point_is_off_by_default`;
  - `wr_647_concentric_arcs_within_tolerance_are_a_range_by_default` (a duplicate);
  - the flag-on variant of `an_arc_and_its_shorter_copy_coincide_over_the_copy`;
  - `issue_948_difference_is_the_same_solid_without_the_nearest_point_flag`;
  - `issue_1979_ring_lip_union_is_valid_at_every_offset_with_the_fix`;
  - `g2_round_minus_its_carried_body_cuts_without_the_nearest_point_flag`;
  - the two `..._joins_its_bin_by_the_witness` tests and
    `a_seam_meridian_crossing_into_the_balls_pole_cuts_with_the_crossing_as_a_curve`
    (solidcore-ops `boolean.rs`), whose flag-off cases the remaining tests run.
  No kept expectation changed, and no golden row moved (283 of 283 units pass unchanged).
- **Not fixed by the flag either way:** the fitted B-spline-loft variant of #18064 (insert
  built by a fitted loft, ym 100 and 200) failed `NotSolid` with the flag on and off alike in a
  local probe on this branch before the field was removed (not committed), so
  retiring the flag does not change it. It stays open as its own work.
