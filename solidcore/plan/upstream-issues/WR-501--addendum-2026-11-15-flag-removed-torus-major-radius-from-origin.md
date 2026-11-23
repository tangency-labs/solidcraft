Updates: WR-501

#### WR-501 (update, 2026-11-15). `torus_major_radius_from_origin` is removed; the major radius is always measured from the torus origin

`Torus::is_nurb_surface_torus` (`surface/torus.rs`) always measures the major radius from the torus origin (the
minor circle's centre projected onto the common line), not from the first sweep circle's centre. The flag is gone, so
the opt-in in the original record and the 2026-11-02 addendum no longer exists. Removed: the field, SolidCraft's
preset line, its entries in the kernel rig's `fixes.json` and in `issue_2260_gn_pipe_refined_range_end.rs`, its flag
lines in the `issue_1945`, `issue_2020` and `wr650` test kernels, and the flag-off branch.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a torus NURB whose tube does not start on the equator is found with its true major radius, not
  `sqrt(R^2 + (r sin v0)^2)`.
- **Goldens:** the `surface.torus` `is_nurb` rows move together with WR-408's (one table in the PR); the committed
  test `wr408_501_504_moved_rows_are_the_exact_torus` (goldens, `units/surface_torus.rs`) checks each found torus
  against the torus it was built from.
- **Tests:** the flag-off halves of `issue_1470_1471_tube_turn_from_bottom_is_found_with_its_radii` (`torus.rs`) and
  `tests/wr501_torus_major_radius_from_origin.rs`, which pinned upstream's `sqrt(R^2 + r^2)`, are removed; the
  expected values of the default path are unchanged.
