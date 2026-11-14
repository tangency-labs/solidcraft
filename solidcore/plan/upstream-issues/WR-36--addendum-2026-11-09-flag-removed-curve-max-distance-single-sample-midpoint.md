Updates: WR-36

#### WR-36 (update, 2026-11-09). `curve_max_distance_single_sample_midpoint` is removed

`Curve::curve_max_distance_between` and `CrvOnSurf::surface_curve_max_distance_between` with `num_samples == 1` sample the interval midpoints (`per = 0.5`) instead of dividing `0 / 0 = NaN`. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::curve_max_distance_single_sample_midpoint`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `wr36_curve_max_distance_single_sample_*` (`curve/curve/p6.rs`) and `wr36_surface_curve_max_distance_single_sample_midpoint` (`curve/crv_on_surf.rs`) assert the midpoint result only; the tests that compared the flag modes on other sample counts are removed.
