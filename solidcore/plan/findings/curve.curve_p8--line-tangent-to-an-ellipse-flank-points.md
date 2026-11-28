# A line tangent to an ellipse came back as two points on the flanks of the touch (#18828)

Unit: `curve.curve_p8`, with `curve.ellipse_p2` and `geom.geom_utility_p1`.

**What failed.** A slanted foot joined to a rounded plate, the foot's top level with the
plate's top, failed in `make_faceuse` ("the related vertex is not an end of the edge
use"). The foot's inner side face cuts the plate's corner cylinder (radius 2) in an ellipse
whose highest point P = (-3, -15.8171570, 10.3) touches the foot's top edge, a line in the
same plane. `Ellipse::intersect_with_line` handles circles only, so the pair went to the
general curve/curve intersector. Its local solves meet the double root of a tangency and
stop on both sides of it, where the two curves are within the solver's zero of each other:
it returned two point solutions 2.68e-5 and 2.44e-5 from P (the curves 3.2e-11 and 2.7e-11
apart there) and none at P. The ellipse's in-plane radius of curvature at P is 11.2, and
the gap between the curves grows as the square of the distance from P over twice that
radius, so it is still only 3.2e-11 at 2.68e-5. One flank point became the end of a 2.68e-5
piece of the ellipse whose ends the Boolean took for one proto vertex; the self-loop it
made was split into two vertices for one point and the merge failed. No later pass of the
intersector joined the two points: `contacts_seen_through_tolerance` (WR-647) reads range
solutions only.

**The rule.** Two consecutive point solutions of a line and an ellipse that is no circle
are the line's one touch with the ellipse when the line lies in the ellipse's plane and
touches it by the circle's contact rule, read on the ellipse's support function
(`line_coplanar_ellipse_touch`). With `n` the line's unit normal in the plane, pointing
away from the centre, `c` the line's distance from the centre and `h = √((a n·x)² + (b
n·y)²)` the distance of the ellipse's tangent line with normal `n`, the ellipse lies in the
strip `|n·(p - centre)| <= h` and meets its edge only at the support point `centre + (a²
(n·x) x + b² (n·y) y) / h`. For a circle `h` is the radius and the support point the
centre's drop onto the line, so these are the circle's `d` against `r`
(`line_coplanar_circle_intersect_distinct`). The line touches when `c >= h` (it does not
enter the ellipse, so it cannot cross it twice) or when the overlap `h - c` is within the
contact tolerance the caller passed (`within_contact_tol`, the system zone tolerance on the
Boolean's path) and the whole ellipse is not. A caller with no contact tolerance still
gets `within_contact_tol`'s allowance for rounding, so a tangent line touches whichever way
the last bits of `c` and `h` fall. The touch is the support point's drop onto the line,
dropped onto both curves; it must lie between the two flank points on both curves (or at
one of them) and within the intersection tolerance of both
(`Ellipse::touch_between_line_flanks`, `Curve::flank_points_of_an_ellipse_touch`, run
after `cull_spurious_points` and before `contacts_seen_through_tolerance`). After a pair is
replaced, the touch is tried against the point solutions on both sides of it, and each
joins it with no further test of where it lies. That is sound because the line does not
cross the ellipse twice and the distance from a point of a line to a convex region is convex
along the line: the points of the line within the tolerance of the ellipse are one stretch
about the touch, and every point solution is one of them. When the rule or a point solve
fails, the pair stands.
Every other answer of the general intersector stands.

**What it does not cover.** A touch at the ellipse's seam, whose flank points lie at the two
ends of the parameter range, is left as two points. So is the same stall at a tangency
between other kinds of curve (a line and a spline, a circle and an ellipse, two ellipses):
this rule proves the touch from the ellipse's support function, which those pairs do not
have.

**An earlier, wider version.** Answering every line in an ellipse's plane in closed form
inside `Ellipse::intersect_with_line` fixed the join too, but moved five golden units that
record those intersections, among them `surface.plane_p2` gen1491: a plane's domain box
trims an ellipse at tolerance 0.5, and the circle's short-chord rule (a chord no longer
than ten tolerances whose sagitta is within one) merged two real crossings, 0.29 and 0.08
deep, into one point each, so the trim lost the arc. The narrow rule only joins two point
solutions the line provably cannot make as two crossings.

**Result.** The join is one body of 189.92876504 mm³; the volume worked out by hand is
189.92876504068 (kernel case `booleans/18828-slanted-foot-flush-with-joined-rounded-plate`).
Tests: `flank_points_of_a_line_tangent_to_an_ellipse_are_its_touch` (`curve/curve/p8.rs`,
which also runs the intersector on the join's ellipse and line and fails without the pass),
`a_line_touches_an_ellipse_only_within_the_contact_tolerance_of_tangent` and
`a_near_circle_ellipse_touches_where_its_circle_does` (`geom/geom_utility/p1.rs`).
