# WR-560: the UV trim's radius test (13) is the edge's 3D curve's, not the trim's

Addendum to WR-560, from tangency-labs/solidcore#2785 (2026-11-07).

WR-560's fix maps a UV trim by its face's surface so tests 13 and 15 measure lengths on the
surface rather than in raw (u, v). For test 15 that stands. For test 13 the quantity is still not
the model's: the constant metric removes the surface's anisotropy, but what remains is the
*image* of the pcurve, and that image carries the pcurve's own bends, which the 3D curve does not.

Measured on `crates/solidcore/tests/kernel_cases/crashes/2992-join-disk-and-36-twisted-loft-
blades`: 74 test-13 reports on the corpus's union-all case, all of them UV trims, none of them an
edge's 3D curve. The one that decides it reads radius `7.79e-4` (mapped metric) / `8.50e-4` (the
true image `S(u(t), v(t))`) on a mapped trim `42.67` across, against `4.27e-3`. Its image is G1 but
the pcurve's `v'` jumps 1.54x at a surface v knot where `|S_v|` jumps by the same factor, so the
image is C1 and not C2 there, and the sub-tolerance "radius" is a sub-nanometre S-bend: the image's
tangent swings `1.8e-2` rad and returns within `3e-5` mm.

The port now runs tests 13/14 on the edge's 3D curve only, and not on a UV trim
(`BSplineCurve::assert_valid`), so WR-560's `issue_891` trim is accepted because a trim's radius is
not tested at all rather than because the metric makes it a circle. `sm_TrimMetric` and
`sm_MappedTrim` stay for test 15. The full record, with the tables and the corpus run, is
`plan/findings/nurbs.b_spline_curve_p3--issue-2785-radius-tolerance-is-a-length.md`.
