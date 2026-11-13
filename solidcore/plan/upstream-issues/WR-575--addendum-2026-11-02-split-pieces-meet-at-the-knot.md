Updates: WR-575

#### WR-575 (update, 2026-11-02). Both pieces of a split end at the knot

The 2026-10-31 rule ended a trim at the interior knot only when the kept piece's own end span was a pole, so it depended on which side of the end was kept. An edge split just past a knot then had its first piece end at the knot and its second start at the split, and the UV trims, split at the first piece's end, no longer matched the second edge's interval: #1529's tubes joined with a gap of 1e-6 or 1e-7 failed `SmEdgeuse` tests #25 and #26 with `Kernel::fixes.bspline_trim_keeps_rational_end_spans` on (#2424). A UV curve could also move its end to a knot of its own while its edge didn't.

With the flag on, a model curve's trim end whose span to an interior knot collapses to a pole (its control points within `SM_ZONE_TOL_3D / 10`) is now that knot whichever side is kept, before the trim, so `TrimMode::IntervalOnly` gives the same interval. A UV curve's ends are its edge's: its pole end span is cleaned as upstream (as WR-441 does since #2129), not moved. With the flag on, the tubes' check at 1e-6 and 1e-7 reports nothing the flag-off check doesn't, and at 1e-6 the circle's `SmEllipse` #1/#2 go too. The 1e-5 case is unchanged.

With every fix on at 1e-6, `SmBSplineCurve` #15 remains. It comes from WR-680's exact UV trim, which the flag reaches by keeping the ellipse's NURB exact (#2427). The fork still has no rule here (2026-10-30 addendum). Tests: `nurbs::b_spline_curve_n_lib::tests::issue_2424_both_pieces_of_a_split_end_at_the_same_parameter`, `wr575_tube_ends_join_adds_no_check_failure_at_any_gap` in `crates/solidcore/tests/wr579_near_coincident_tube_ends_join.rs`.
