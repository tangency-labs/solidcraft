#### WR-21139. A mirrored circle or ellipse becomes the rational B-spline of its NURB
- **Severity:** Wrong result (the analytic type of a mirrored body's round edges is lost)
- **Area:** Topology (mirror and transform)
- **Location:** `SmBSplineCurve::CreateMirrorCurve`, which `SmEllipse` and `SmCircle` inherit;
  `SmBrep::Mirror` and `SmBrep::Transform` with a left-handed matrix call it for each edge curve.
  solidcore `curve/curve/p2.rs` `Curve::create_mirror_curve`.
- **What happens:** the mirror of an ellipse or circle is a plain B-spline over the ellipse's NURB:
  a rational quadratic with weights 1/sqrt(2). The geometry is exact, but the curve is no longer
  a circle. Each round edge of a mirrored body is such a curve, and STEP export writes it as a
  `B_SPLINE_CURVE` / `RATIONAL_B_SPLINE_CURVE` instead of a `CIRCLE`. Readers that recognise
  circles from the curve type no longer see a hole's diameter. Surfaces are not affected:
  `SmBrep::Mirror` makes the mirrored surfaces analytic again (`CopyAndAddAnalytics`). Lines
  were fixed by WR-784, but not ellipses and circles.
- **Trigger:** a cylinder of radius 10 and height 100 at x = 50, mirrored in the plane x = 0
  (`topology/brep/mirror.rs` `mirrors_a_cylinder_s_circles_as_circles`; in SolidCraft,
  `solid.mirror` of a cylinder, then STEP export).
- **How found:** a SolidCraft user's STEP export of a mirrored part.
- **Fix (always on, no flag):** an ellipse mirrors as an ellipse (`Ellipse::create_mirror_curve`).
  Its origin, X axis and Y axis are mirrored, and its radii, angular domain and sides are kept.
  An existing NURB is mirrored with it, and the polar conversion is rebuilt. The mirrored X and Y
  make a right-handed placement whose normal `X × Y` is the reverse of the mirrored normal.
  Each angle and each NURB parameter maps to the mirror of the same point, so edge intervals and
  pcurves stay valid. Parabolas and hyperbolas still mirror as B-splines.
- **Confirmed:** no C++ run; read from solidcore's port, and measured in solidcore (the test
  above fails without the fix).
