Updates: WR-552

#### WR-552 (update, 2026-11-09). `revolve_arc_touching_axis_is_horn_torus` is removed

A profile arc of a circle that reaches the revolve axis within the caller's tolerance is revolved as the horn torus (major radius equal to minor), not a spindle torus whose minor circles cross the axis (the fork's c4e378d); the body passes SMLib's check. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::revolve_arc_touching_axis_is_horn_torus`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/wr_552_revolve_arc_touching_axis.rs`.
