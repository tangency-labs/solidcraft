Updates: HANG-52

#### HANG-52 (update, 2026-11-09). `ssi_start_loop_trace_budget` is removed

The traces of one start-point loop of `DoIntersection` share one bound, `MAX_START_LOOP_TRACE_STEPS`; a trace past it fails the intersection with the too-many error. The default kernel runs the filleted-box defeature of the record in under a minute (release). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::ssi_start_loop_trace_budget`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `intersect::surface_intersector::p5::tests::do_intersection_bounds_its_start_point_loop_traces` (a bound of one step refuses; the real bound still finds the sphere/plane circle) and `p4::tests::trace_steps_are_bounded_by_the_start_point_loop_budget`; `tests/hang52_defeature_trace_budget.rs` (ignored, release only) no longer needs a flag.
