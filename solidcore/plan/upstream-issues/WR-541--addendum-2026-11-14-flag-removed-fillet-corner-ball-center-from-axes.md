Updates: WR-541

#### WR-541 (update, 2026-11-14). `fillet_corner_ball_center_from_axes` is removed

`CalcSphereFilletGeom` centres the corner ball where the two cylinders' axes meet, not at the trace points' centres, whose last solve stops a step short of the crossing (the fork's bc6ddf9). The parallel-axes refusal before it goes with the old path. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_corner_ball_center_from_axes`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** the corner ball tests under `blend::fillet_solver` and the rig's corner cases.
