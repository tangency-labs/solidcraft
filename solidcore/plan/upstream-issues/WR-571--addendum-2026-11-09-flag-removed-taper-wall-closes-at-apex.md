Updates: WR-571

#### WR-571 (update, 2026-11-09). `taper_wall_closes_at_apex` is removed

`curvature_keeping_walls` (`sm_CreateCurvatureKeepingWalls`) follows the section up: a wall that closes before the far end ends at its apex and its neighbours meet from there up (a wedge's tip, a slot's sides face to face), where upstream refuses the draft with `WallCloses`. The seam-by-seam path that refused them is removed. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::taper_wall_closes_at_apex`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/wr_571_draft_wall_closes_at_apex.rs` (a wedge, volume 72.883284979193 with 6 faces, and a slot, volume 213.984095421783 with 10 faces); the test that expected the refusal is removed.
