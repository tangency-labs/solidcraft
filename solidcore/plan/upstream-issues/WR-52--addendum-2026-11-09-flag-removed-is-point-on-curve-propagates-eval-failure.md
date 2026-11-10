Updates: WR-52

#### WR-52 (update, 2026-11-09). `is_point_on_curve_propagates_eval_failure` is removed

`SurfaceIntersector::is_point_on_curve` and `is_point_on_tsect_curve` return a failing `EvaluatePoint`, `DropPoint` or Hermite `LocalPointSolve` status as `Err`; upstream maps it to `TRUE` (`SER` inside an `SmBoolean`). The `or_true` helper is gone. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::is_point_on_curve_propagates_eval_failure`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `intersect::surface_intersector::p1::tests::wr52_*` (degenerate and non-degenerate failing curves, the cached variant, skipping the broken curve, valid curves and traced spans) assert the propagated error only.
