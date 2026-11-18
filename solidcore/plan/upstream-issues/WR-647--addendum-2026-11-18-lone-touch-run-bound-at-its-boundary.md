Updates: WR-647

#### WR-647 (addendum 2026-11-18): the lone-touch run bound, pinned at its boundary

- **Status in the port:** This clarifies the touch convention in `WR-647--addendum-2026-11-18-crossings-on-merged-ranges-flag-narrowed.md` (#18392), from review r9 (#19118); no golden result changes.
- **The run bound is necessary, not enough on its own:**
  - `touch_is_alone` (`curve/curve/p8.rs`) requires the proven run where the curves are within the scaled zero `z` of each other to be within the tolerance (`run_length <= tol`). If it is not, the stretch keeps upstream's range.
  - Passing this check does not certify a point by itself. The isolation checks still apply: `least >= -eps`, or else `one_contact`, which refuses rational curves. Failing those, `curves_meet_at_most_once` must hold, or the stretch keeps its range.
  - So a touch of a line is one point only when every one of these holds.
- **The bound is sufficient, not necessary:** `curve_speed_bound` uses the largest span speed and the largest point length over the whole spline, so its slack depends on how the speed varies and a run under the tolerance can still be refused (keeping its range).
  - Measured in review r9 on the `flat_then_bending` fixture (outer curvature 2.2e-2, tolerance 1e-3): a true run of 5.15e-4 (0.5 times the tolerance) was refused, and runs of 2.65e-4 and 1.07e-4 were certified.
  - That is one fixture; the slack elsewhere differs.
- **Rounding:** `curve_speed_bound` inflates by `gamma(16)`, covering its own roundings (about 13). It no longer borrows the run bound's margin.
- **"Never meet", made precise:** curves within tolerance that never come within the zero of each other over a run the tolerance resolves keep their ranges. Curves that never meet exactly, but stay within `z` over a run under the tolerance, are one point, like an exact touch.
- **Tests:** `wr_647_a_run_within_the_zero_resolved_by_the_tolerance_stays_a_range` now pins the cut from both sides (review r9):
  - runs within the zero of 4.0e-3 (h 1e-3) and 1.015e-3 (h 2.5e-4) at tolerance 1e-3, with dr 1e-13 and 0, in both orders, stay ranges;
  - a run of 1.07e-4 (h 2.5e-5) is one point at the origin on both curves, in both orders;
  - loosening the cut to `run_length <= 30 * tol` fails the test.
