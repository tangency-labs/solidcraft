Updates: WR-504

#### WR-504 (update, 2026-11-15). `torus_iso_circle_axis_from_midpoint` is removed; a minor iso circle starting on the axis always takes its direction from its midpoint

`Torus::create_iso_parametric_curve` (`surface/torus.rs`, `iso_curve`) gives a minor iso circle whose start lies on
the axis the direction of its midpoint, and returns a failed midpoint evaluation. The flag is gone, so the opt-in in
the original record no longer exists. Removed: the field, SolidCraft's preset line, its entries in the kernel rig's
`fixes.json` and in `issue_2260_gn_pipe_refined_range_end.rs`, its flag lines in the `issue_1945`, `issue_2020` and
`wr650` test kernels, and the flag-off branch, which reused the zero start vector and gave the circle a NaN axis.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a horn torus's minor iso circles have a unit axis and an analytic domain.
- **Goldens:** 2 `surface.torus` rows move (`horn_half_minor_iso_from_axis_0.0` and `_0.5`), from NaN circles to the
  minor circle. The committed test `wr408_501_504_moved_rows_are_the_exact_torus` checks the circle: a unit x axis in
  the torus's xy plane, centre R along it, radius r, the torus's v interval length, and the NURB's ends and middle on
  it.
- **Tests:** the flag-off half of `issue_1514_horn_torus_minor_iso_circle_takes_direction_from_midpoint`
  (`torus.rs`), which pinned the NaN axis, is removed; the default path's expected values are unchanged.
