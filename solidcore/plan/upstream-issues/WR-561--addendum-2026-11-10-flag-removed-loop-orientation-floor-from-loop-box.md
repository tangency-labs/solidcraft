Updates: WR-561

#### WR-561 (update, 2026-11-10). `loop_orientation_floor_from_loop_box` is removed

`SmFace::ComputeLoopOrientation`'s 3D path holds a loop's dimensionless area to a floor relative to the loop's own bounding box (fork tangency-labs/smlib 54ca3e0), so a loop far smaller than its face is oriented. That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. Every goldens unit (283) gives the same output at this change as at main `87400c80e`, run on Windows. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** `issue_1523_a_tiny_hole_is_oriented_against_its_outer_loop`. The default-off test, `issue_1523_a_tiny_hole_is_not_oriented_by_default`, is deleted.
