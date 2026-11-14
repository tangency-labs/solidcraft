Updates: WR-420

#### WR-420 (update, 2026-11-14). `fillet_corner_ball_patch` is removed

The fillet solver's try-chain makes the corner ball's patch after the torus when a round's centre line collapses to a point. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_corner_ball_patch`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/wr669_exact_round_classes.rs` and the corner ball tests.
