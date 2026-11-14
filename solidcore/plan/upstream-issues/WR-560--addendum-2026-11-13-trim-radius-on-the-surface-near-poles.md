# WR-560: a trim's radius is certified on its surface over the whole trim

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
image `S(c(t))` is at least the tolerance over the whole trim. Over the zero-speed stretches
at its ends (WR-668), where the radius vanishes with the speed, it must be at least half the
growth line WR-668 reads there, `(tol / stretch) |t - t_end| / 2`, at the near edge of each
of at most 52 dyadic pieces (`tol 2^-(j+2)` over the piece `stretch 2^-(j+1)` to
`stretch 2^-j` from the end). A genuine zero-speed end has its radius on the line to first
order, so the full line would leave it no margin. This margin is a DEFINITION of the defect
inside a zero-speed stretch, chosen in review. It is looser than WR-668's own check, which wants at least
`tol / 2` at the stretch's far end, where the outermost piece asks for `tol / 4`. The
pieces stop at a cut from the end, and the last piece's near edge is the cut. The cut is
the largest of `ulp(t_end)` and `stretch 2^-52` (below which the pieces would round onto
the end) and the parameter length over which the trim covers a UV length of `L / 4`,
`L = 1024 ε |P|_max`, to second order. That length is needed because the enclosure's
coefficients carry absolute widths of a few `ε |P|`, so shorter pieces cannot clear
(measured: on a cubic with `P0 = P1`, pieces from about 1e-7 of the end inward do not
clear even an eighth of the line). The cut is capped at the first knot from the end, so
that the sliver within it is one polynomial piece. The sliver's UV diameter is then
bounded from all of its power coefficients in rounded intervals and must be at most `L`,
at the trim's own coordinate rounding; otherwise the stretch is not certified (for
instance where `ulp(t_end)` binds). Within the sliver the radius vanishes with the speed
and no positive bound clears it. The sliver's 3D diameter is also certified, from its UV
diameter and an interval enclosure of the surface's Jacobian over its UV box, to be at most
`1024 ε` of `max(net_max, J · trim_max)` (the surface net's largest coordinate, or the
Jacobian bound times the trim's largest coordinate); that an image at the model's own rounding
holds no corner the trim could represent is ARGUED. A knot of full multiplicity inside the
trim (a possible C0 corner, which the pieces, cut at the knots, would not see) refuses the
bound unless its two tangents are parallel to rounding (an interval cross product within
64 ε of their lengths' product, ARGUED as rounding: a G1 join); a knot of multiplicity
above the degree refuses it regardless. The bound composes each polynomial piece
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

Known gap, unchanged from main: the constant metric also understates a turn where a step
in u is shorter on the surface than at the trim's middle, and only solutions the mapped
analysis finds below the tolerance are measured on the surface. A trim with such a corner and
no solution below the tolerance is not reported, on main as here. A trim that also has a
turn this change would pass over is reported: the pass-over needs the whole trim to clear on
the surface, and the corner does not. `a_kink_on_the_ball_off_a_turns_parameter_is_not_cleared`
in `p3.rs` is that case: on a r 6 sphere, a trim whose middle lies at latitude 70 degrees
turns onto a latitude circle 0.01 rad from the pole and back down a meridian after 0.06 rad
of longitude. Its corner has radius about 5e-6 on the ball but about 4.2e-3 mapped. Test 13
reports it. Measuring every trim on its surface would close the rest of the gap and is left
as a follow-up.

Code: `radius_clears_on_surface` (`crates/solidcore/src/nurbs/b_spline_curve/trim_image.rs`)
and the pass-over in `BSplineCurve::assert_valid_with`
(`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
