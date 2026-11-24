Updates: WR-342

#### WR-342 (update, 2026-11-15). `offset_clip_right_side_out` is removed; an inset face is always clipped right side out

`BuildOffsetOfFaceuse` (`offset/offset_executive/p1.rs`) always clips a face that does not fit its offset surface's
domain from its counterclockwise loops, and reads the clipped faces' loops in the face use's sense. The opt-in
described in the original record no longer exists. Removed: the field, SolidCraft's preset line, its `fixes.json`
entry, the flag-off branches, and the flag settings in the shell tests.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** no row of any unit moves.
- **Tests:** `wr_342_vase_shells_closed_with_the_fix` runs on the default kernel (the closed-form volume of the
  shelled vase). The test of the flag-off refusal (`wr_342_default_refuses_the_open_shell_and_retains_the_vase`) is
  removed: that path no longer exists.
