Updates: WR-37

#### WR-37 (update, 2026-11-11). `bspline_simple_offset_matches_parameterization` is removed

`BSplineCurve::create_simple_offset` (`nurbs/b_spline_curve/p2.rs`) now always passes `false` for `ApproximateCurve`'s `bOptCreateAnalytics` and forwards `bOptMatchParameterization` in its own slot. That is the only path now, and the flag is gone. Upstream's call, which passes the match flag in the analytics slot and `FALSE` for matching, is no longer reachable in the Rust kernel; the C++ defect is unchanged in v1.0.1. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and its test lines.

- **Results:** see the retiring PR for the goldens run on a main export. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** `wr37_bspline_simple_offset_matches_parameterization` (the matched offset of the non-uniform cubic within 1.5 tol of the exact offset at 101 points) and `wr37_linear_bspline_and_ellipse_simple_offset_with_match_parameterization` run on the default kernel. `wr37_bspline_simple_offset_unmatched_lies_on_the_offset` replaces `wr37_bspline_simple_offset_unmatched_mode_unchanged_by_opt_in`, which compared the two settings: without matching the call is upstream's, and the test checks that offset within 1.5 tol of the exact offset locus. The upstream half of the first test is deleted.
