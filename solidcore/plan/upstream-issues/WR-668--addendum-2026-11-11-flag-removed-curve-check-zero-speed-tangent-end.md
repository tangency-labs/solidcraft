Updates: WR-668

#### WR-668 (update, 2026-11-11). `curve_check_zero_speed_tangent_end` is removed

`SmBSplineCurve::AssertValid` (`nurbs/b_spline_curve/p3.rs`) now always takes a zero-speed tangent end of an edge's curve or UV trim: test 3 passes over the coincident first control points and tests 13/14 pass over a radius below the tolerance within the end's growth stretch. The edge and edge-use checks (`topology/brep/validation/assert_valid/edge.rs`) always build the edge's `EdgeEnds`, and `edge_ends` returns them rather than an `Option`. That is the only path now, and the flag is gone. Upstream's check, which reports both, is no longer reachable in the Rust kernel. A curve on no edge is checked as upstream. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** see the retiring PR for the goldens run on a main export. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** the WR-668 unit tests in `p3.rs` and `edge.rs` check the default kernel. `wr_668_flag_off_ignores_the_edge` and the flag-off halves are deleted. `zero_speed_end_fixtures_on_no_edge_report_as_upstream` (goldens unit `nurbs.b_spline_curve_p3`) replaces the test that compared the fixtures with the flag off and on.
