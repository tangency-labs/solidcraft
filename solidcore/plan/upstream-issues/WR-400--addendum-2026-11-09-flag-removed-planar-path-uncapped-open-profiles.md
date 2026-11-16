Updates: WR-400

#### WR-400 (update, 2026-11-09). `planar_path_uncapped_open_profiles` is removed

`SweepProfilesAlongPath` asks `create_curve_sweep` for caps only for a profile that closes; upstream always asks, an open profile bounds no planar region, its cap fails and `CreateCurveSweep` returns before stitching its walls. `cap_profile_ends` is gone. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::planar_path_uncapped_open_profiles`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `primitive::primitive_creation::p2::planar_path::tests::issue_837_open_profile_sweeps_without_caps`.
