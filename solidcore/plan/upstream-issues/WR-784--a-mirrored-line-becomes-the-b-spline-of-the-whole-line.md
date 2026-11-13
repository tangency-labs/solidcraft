#### WR-784. A mirrored line becomes the B-spline of the whole line
- **Severity:** Wrong result (a boolean against the mirrored body)
- **Area:** Topology (mirror and transform)
- **Location:** `SmBSplineCurve::CreateMirrorCurve` (`source/SMLib/src/SmBSplineCurve.cpp:2330`-`2362`),
  which `SmLine` inherits; `SmBrep::Mirror` and `SmBrep::Transform` with a left-handed matrix call
  it for each edge curve. solidcore `curve/curve/p2.rs` `Curve::create_mirror_curve`.
- **What happens:** the mirror of a line is a plain degree 1 B-spline over the line's NURB. A line
  read from a record, or made by `CreateCanonical`, is infinite: its NURB runs over its whole
  analytic domain, knots at `-SM_INFINITE_PARAMETER` and `SM_INFINITE_PARAMETER` (1234567), its
  control points that many edge lengths out (about 1.8e7 mm for a 14.6 mm edge). Each straight
  edge of a mirrored body is such a curve. A boolean of the mirrored body with a spline face it
  does not touch takes the wrong side: in the trigger the loft minus the box is nothing, the loft
  and the box intersect in the whole loft, and their union is refused (status 1001). The same box
  with each edge curve a line, or the same B-spline cut to the edge's interval, gives the right
  answers. Surfaces are not affected: `SmBrep::Mirror` makes the mirrored surfaces analytic again
  (`CopyAndAddAnalytics`), curves it does not.
- **Trigger:** a loft through two closed cubic splines with two box openings in its side, and a
  box extruded from a rectangle, read back from its record and mirrored across x = 0 into the
  second opening, 0.2 mm clear of the loft (`solidcore-ops/tests/boolean.rs`
  `a_box_mirrored_into_an_opening_of_a_spline_loft_stays_clear_of_it`).
- **How found:** solidcraft#15862 (`inspect.interference` reports a mirrored box as overlapping a
  loft by the loft's whole volume).
- **Fix (always on, no flag):** a line mirrors as a line (`Line::create_mirror_curve`): its point
  and unit vector mirrored, its domain, scale and sides kept, an existing NURB mirrored with it,
  so each parameter maps to the mirror of the same point and the edge intervals stay valid.
- **Confirmed:** no C++ run; read from the pinned source, and measured in solidcore (the test
  above fails without the fix).
