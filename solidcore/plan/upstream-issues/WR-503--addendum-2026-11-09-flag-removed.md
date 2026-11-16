Updates: WR-503

#### WR-503 (update, 2026-11-09). `fillet_trace_rejects_short_step` is removed

The guard in `FilletIntersector::compute_next_point` (an unclipped step that lands less than half a
step from the previous point is another root of the equations, so the point is demoted and the
trace stops) is now unconditional, and the flag is gone. SolidCraft already ran with it on.

- **Results:** none change in the goldens. The default kernel now has the guard, so a rail no
  longer doubles back where `StepSolve` lands on another root.
- **Tests:** `blend::fillet_intersector::tests::wr503_next_point_rejects_unclipped_short_step`
  (an unclipped step landing a fifth of the step away is refused; one landing at nine tenths, and
  a clipped one, are accepted). The end-to-end plate of the record is not reproduced by the port
  (`general--flag-reachability-wr503-short-step-not-reproduced`), so the test covers the predicate
  at the call that decides. The tests that set the flag drop the setting.
