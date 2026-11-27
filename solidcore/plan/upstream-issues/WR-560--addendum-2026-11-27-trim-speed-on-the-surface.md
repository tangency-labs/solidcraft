# WR-560: a trim's speed (test 11) is measured on its surface too

Updates: WR-560

WR-560 maps a face's UV trim by the Cholesky factor of its surface's first fundamental
form at the trim's middle before tests 13 and 15. Test 11, the first-derivative test
("Bad SpeedPt", `SmBSplineCurve.cpp`, `AssertValid`), still read the trim in raw (u, v).
A speed is a length per unit of parameter, so test 11 has the same defect: on a face whose
two parameters have very different lengths on the surface, a trim reads as nearly stopped
where it runs along the parameter whose unit step is long, although on the surface it moves
at its usual speed. The fork maps tests 13 and 15 only.

Trigger (MEASURED, main 235991e554): the upper half of a torus (centre circle Ø1000, tube
Ø1), both edges of its flat face rounded with 0.05, less a Ø0.1 notch 0.25 deep on the flat
face's outer rim. The boolean's result failed test 11 on a cubic UV trim of the outer round
(a surface of revolution), over a parameter range of 0.218:

| | raw (u, v) | mapped (lengths on the surface) |
|---|---|---|
| box diagonal | 0.967 | 0.128 |
| tolerance (box / range / 10000) | 4.44e-4 | 5.86e-5 |
| slowest interior speed the search finds | 3.54e-4 | 0.969 |

Change (not upstream, no flag): test 11 of a UV trim runs on the same mapped trim as test
13: the search (`SM_CP_MINIMIZE_FIRST_DERIVATIVE`), the speed evaluated at each solution,
the tolerance and solution spacing from the mapped box (`bounded_scale(mapped_size,
size_t)`), and the closed-form speed bound of tangency-labs/solidcraft#11574 taken on the
mapped trim's spans. The mapping is computed once, before test 11, and test 13 reads it as
before (its report stays where it was). A curve that is no face's trim, an unbounded one, or
one whose surface has no metric at either point is tested as before; so is a trim whose
mapped copy cannot be sized (it fails test 13, as WR-560 has it). Test 12 (3D) is unchanged.

Limits, the same as WR-560's for test 13: the constant metric is exact on a plane only.
Near a singular point of the surface it overstates steps along the collapsing direction, so
a trim whose image truly slows there can read faster mapped. A proved lower bound on the
speed of the image `S(c(t))`, from its composed coefficients as `radius_clears_on_surface`
bounds its radius, would close that gap and is left as a follow-up.

Tests: `issue_23062_uv_trim_speed_is_measured_on_its_surface` in
`crates/solidcore/src/nurbs/b_spline_curve/p3.rs` (the exact trim of a r 0.4 circle on a
plane 100000 x 1: test 11 fires in raw (u, v), not on the plane), and
`a_notch_through_a_thin_round_on_a_large_half_torus_is_cut` in SolidCraft's
`crates/engine/src/boolean_regression_tests.rs` (the trigger, its volume against the notch
integrated over the rounded section).

Code: `BSplineCurve::assert_valid_with` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
