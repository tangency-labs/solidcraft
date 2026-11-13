# WR-560: a trim's radius is certified on its surface over each turn's neighbourhood

Updates: WR-560

WR-560 maps a face's UV trim by the Cholesky factor of its surface's first fundamental
form at the trim's middle before test 13. That constant map is exact on a plane only.
Near a singular point of the surface it measures the trim at the wrong scale: at a
sphere's pole the u direction collapses, and the exact trim of a smooth curve passing a
distance `d` from the pole turns through up to half a period in u over a few times `d`.
Mapped by the metric at the trim's middle, such a turn can read as a cusp far below the
tolerance although its image on the surface is smooth.

Change (not upstream, no flag): a test-13 solution below the tolerance on a mapped trim is
passed over only where a certified lower bound on the radius of curvature of the trim's
image `S(c(t))` is at least the tolerance over the solution's whole neighbourhood, out to
the neighbouring solutions (else the trim's ends). The bound composes each polynomial piece
of the trim with each polynomial patch of the surface in rounded interval arithmetic and
bounds `|g'|^3 / |g' x g''|` from the composed coefficients (no sampling); pieces that do not
clear are halved, breadth first, at most 4096 pieces and 64 halvings each, no recursion. A
NaN or infinite bound, a rational trim and anything the budget does not prove report as
before. Curves that are no face's trim, 3D curves (test 14) and unmapped trims are unchanged.

Measured on the trigger that motivated it (a half roll, r 48..50 about z, less a r 6 ball
whose poles lie about the zone tolerance off the roll's outer face, #15994): the traced
trims near the poles have image radii of `4e-5` to `1e-4` against an exact intersection
curve of radius about 6 there, and up to 4 degrees of tangent error at 6.25e-7 of position
error. That is a defect of the surface/surface trace's UV fit near a pole (its span test
bounds position only), not of the exact curve, and the bound reports it: those cuts stay
refused until the fit is fixed (follow-up of #15994).

Known gap, unchanged: the constant metric also understates a turn where a step in u is
shorter on the surface than at the trim's middle. On a r 6 sphere, a trim whose middle lies
at latitude 70 degrees and which turns back down a meridian 0.01 rad from the pole after
0.06 rad of longitude (`a_kink_on_the_ball_off_a_turns_parameter_is_not_cleared` in
`p3.rs`) has a corner of radius about 5e-6 on the ball, but about 4.2e-3 mapped, above the
tolerance 1.0e-3, so test 13 never sees it, on main as here. Only solutions below the
tolerance are measured on the surface. Measuring the whole trim on its surface would close
that gap and is left as a follow-up.

Code: `radius_clears_on_surface` (`crates/solidcore/src/nurbs/b_spline_curve/trim_image.rs`)
and the pass-over in `BSplineCurve::assert_valid_with`
(`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
