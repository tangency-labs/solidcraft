Updates: WR-407

#### WR-407 (update, 2026-11-12). `sweep_frame_on_planar_path_normal` is removed

The sweep-frame half of WR-407 is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B43, which retires `sweep_frame_on_planar_path_normal` and `sheet_sweep_joins_profile_wires`). `prepare_curve_sweep` (`primitive/primitive_creation/p3/geometry.rs`) always starts a planar path's frame with its z on the plane's normal. The other half, `cover_refuses_folded_continuation`, stays opt-in.

- **Tests:** `issue_434_planar_path_frame_keeps_the_plane_normal_as_the_frame_z` runs on `Kernel::default()`; `issue_434_default_keeps_the_upstream_sweep_frame` is removed. `tests/wr_660_planar_path_pose.rs` no longer switches the frame: both rigid-motion tests hold the bounding boxes to 1e-5, the bound of the removed test that switched it on (on main's default kernel they already met it). Fixture `434-two-overlapping-swept-arc-walls` turns on only `cover_refuses_folded_continuation`.
- **Results:** the goldens rows B43 moves, with their old and new values:
  `plan/findings/sweep.api_brep--b43-two-flags-retired-2026-11-12.md`.
