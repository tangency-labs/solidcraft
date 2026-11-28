Updates: WR-6

#### WR-6 (update, 2026-11-10). `periodic_extent_add_empty_interval` is removed

`PeriodicExtent1d::add_interval` (`SmPeriodicExtent1d::AddInterval`) on an empty extent stores the interval itself. Upstream's sentinel test is always true, so it stores the full period. The interval is now the only path, and the flag is gone. Removed: the field, `add_interval_with_kernel` and `add_interval_opt_in`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `geom.periodic_extent1d` changes in 6 cases, each checked by hand against the forward and backward wrap rules. For example, period 1 with forward (0.2, 0.3) is now [0.2, 0.3], where upstream gives [0, 1]. Backward 0.079 to 1 now spans 0.079, where upstream gives the full period.
- **Tests:** the `wr6_empty_add_interval_*` unit tests in `geom::periodic_extent1d`.
