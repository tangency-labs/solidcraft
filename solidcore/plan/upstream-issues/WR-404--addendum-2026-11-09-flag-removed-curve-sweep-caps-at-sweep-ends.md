Updates: WR-404

#### WR-404 (update, 2026-11-09). `curve_sweep_caps_at_sweep_ends` is removed

`PrimitiveCreation::create_curve_sweep` takes its caps from the side faces at the sweep's ends (`sweep_end_curves`), not from the first and last `count` side faces, which are not the end faces when a path that is not G1 splits every swept surface into one face per piece. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::curve_sweep_caps_at_sweep_ends`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** the existing curve-sweep tests build their caps this way; the branch that took the first and last `count` faces is removed.
