Updates: WR-3

#### WR-3 (update, 2026-11-14). `extent3d_contains_both_segment_endpoints` is removed; both ends are the only test

`Volume::is_line_in_param_domain_simple` (`surface/volume/p1.rs`) always requires both segment ends inside the
natural parameter box of a bounded rectilinear domain (B-spline volumes). A box is convex, so that holds exactly when
the whole segment is inside. The flag is gone, so the 2026-10-31 addendum's opt-in no longer exists. Removed: the
field, SolidCraft's preset line, its entries in the kernel rig's `fixes.json` and in
`issue_2260_gn_pipe_refined_range_end.rs`, and the flag-off branch. The function no longer takes the kernel.
`Extent3d::contains_line_seg` keeps upstream's either-end result for its own callers (none in production).

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a segment with one end outside the box is no longer in the domain.
- **Goldens:** 119 rows move, each a line-in-domain answer from 1 to 0: `surface.b_spline_volume` preds (96, `in_line`)
  and `surface.volume_p2` line_in_in_space (23; the first map is a B-spline with no orient map, so the AND over the
  chain is 0). In each, one end lies outside the first-to-last-knot box and the other inside. The smallest margin is
  1e-7 (z = 1.0000001 against a box ending at 1). No other row of any unit moves.
- **Tests:** `tests/wr3_segment_box_containment.rs` runs on the default kernel. The half that pinned upstream's
  either-end answer is removed; the expected values for the default (both ends) are unchanged.
- **Open:** `Extent3d::contains_point` is a negated OR of `<`/`>` tests, so a NaN coordinate counts as inside. A NaN
  end therefore still passes this test, before and after this change.
