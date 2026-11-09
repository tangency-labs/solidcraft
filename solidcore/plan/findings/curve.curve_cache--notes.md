# curve.curve_cache

Partial (deferred: `GetAnalyticCurve` of lines and arcs needs `SmLine` / `SmCircle` /
`SmCurve::IsArc`), no disagreement. Notes for later units:

- **Co-ports.** Every tessellation reaches `SmFindCurveDistanceExtremaEFO::Evaluate`
  (unit `curve.curve_p4`) and, through `SmBSplineCurve::CalculateBoundingBox` /
  `IsLine`, `SmCurve::IsLine` (unit `curve.curve_p5`). Both are ported here:
  `impl EvalFunction for FindCurveDistanceExtrema` in `curve::curve_cache`, and
  `curve::curve::p5::is_line_base` (B-spline only; the `Curve` dispatch is left to
  `curve.curve_p5`). The `UNPORTED(curve.curve_p5): SmCurve::IsLine` stub of
  `nurbs::b_spline_curve::p2` now calls it; `nurbs.b_spline_curve_p2` (56 cases) and
  `_p3` (188 cases, some now reaching `curve.curve_p7`) un-deferred and pass. Porters of
  `curve.curve_p4` / `_p5`: do not port these twice.
- **Tree data.** `geom::tree::NodeData` gained `Curve(CurveSpanRef)`; a curve tree owns
  its cache's span/parent arrays (`Tree::set_curve_data`) and the end points `GetEnds`
  evaluates (stored at build time, since the tree cannot reach the curve). The curve
  solvers of `curve::curve::p2` read them through `Tree::curve_aux` / `curve_span`.
- **Upstream quirks kept.** `sm_ComputeConstants` stops at the first non-zero polygon
  angle when the angle tolerance is `0` (`dTotalAngle > 0`), so with only a chord
  tolerance the chord height is often not computed; after a successful local solve it
  measures the distance at the *step* parameter, not the solution. `BuildTree`'s parent
  test omits the off-axis and approximation tests the leaf tests use. Tessellation with
  a tiny chord tolerance and `dMinimumParametricRatio = 0` subdivides down to
  `SM_EFF_ZERO_SQRT` intervals (up to ~10^6 spans); the ffi wrapper requires a ratio of
  at least `1e-4`.
