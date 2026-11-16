Updates: WR-791

#### WR-791 (update, 2026-11-16). A curve gives way to the touch only when its control points prove it runs along the boundary

`SurfaceIntersector::runs_along_boundary` decided at sample points: each distinct knot of the curve and each midpoint between two of them had to drop onto the boundary curve closer than the tolerance. A curve that left the boundary between those points was removed: a cubic on the boundary at its ends and its middle, 3 tolerances off it at a quarter, gave way to the touch. That broke "No sampled certificate" (`docs/definitions-and-certificates.md` §7.6).

It is now proved on control points. Each rational Bézier piece of the curve needs its control points within the tolerance of the chord of one piece of the boundary curve, less that piece's own distance from its chord and the rounding of both; the rounding is bounded from the B-spline each was cut from (`blend_corner::on_face::pieces_rounding`) and the sum rounded up. Whatever the check cannot prove keeps the curve, among them a curve along a boundary piece that bends away from its chord by the tolerance (an arc), which gave way before.

- **Results:** none of the goldens or the boolean test of #11749 move (the boundary there is a line).
- **Tests:** `intersect::surface_intersector::p3::tests::a_crossing_curve_along_the_boundary_of_a_deferred_contact_gives_way_to_it` (the cubic between its samples, a cubic of two pieces within half the tolerance, a line past the boundary's end).
