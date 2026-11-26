Updates: WR-660

#### WR-660 (update, 2026-11-09). `planar_path_profile_in_path_frame` is removed

`position_profile_curves` places the profile in the whole path's own plane frame at the path's start and maps it back, for paths with a plane outside XY; paths parallel to XY and straight paths keep upstream's arithmetic. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::planar_path_profile_in_path_frame`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/wr_660_planar_path_pose.rs` (rigid-motion invariance of the planar-path sweep, with and without `sweep_frame_on_planar_path_normal`); the tests that pinned the pose-dependent upstream result are removed.
