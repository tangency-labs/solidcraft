Updates: WR-647

#### WR-647 (addendum 2026-11-18): the touch run bound does not replace the isolation checks

- **Correction:** the touch convention in `WR-647--addendum-2026-11-18-crossings-on-merged-ranges-flag-narrowed.md` (as narrowed in #19118) calls the run bound sufficient. That should not be read as certifying a point on its own.
  - `touch_is_alone` (`curve/curve/p8.rs`) *requires* `run_length <= tol`, the proven run where the curves are within the scaled zero of each other.
  - The isolation checks still apply after it: `least >= -eps`, or else `one_contact`, which refuses rational curves. Failing both, `curves_meet_at_most_once` must hold. Otherwise the stretch keeps upstream's range.
  - So a touch of a line is one point only when the run bound and one of these isolation certificates both hold.
- **The bound's slack is not a general constant.** `curve_speed_bound` takes the largest span speed and the largest point length over the whole spline, so its slack depends on how the speed varies. Review r9 measured it on one fixture only: on `flat_then_bending` (outer curvature 2.2e-2, tolerance 1e-3), a true run of 5.15e-4 was refused, while runs of 2.65e-4 and 1.07e-4 were certified. The "about 2x" in the narrowed addendum describes that fixture, not the bound in general.
- **No result changes:** this is documentation only. The one-point guard row of `wr_647_a_run_within_the_zero_resolved_by_the_tolerance_stays_a_range` now also checks that both returned parameters give the contact at the origin, in both orders (#19118 review).
