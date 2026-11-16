# nurbs.b_spline_curve_p2 (SmBSplineCurve part 2)

No Rust-vs-C++ disagreement: 2011 of 2011 non-deferred difftest cases bit-identical (328
fixtures: the six `pt_TestFiles` `.smc` curves through every op and read back with
`ReadFromDB`, the `my_test_reversed_line_join` line pairs, `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` style joins, arc/line joins, Bezier box special cases and edge
cases, plus 2000 generated); a 20,328-case run was also clean. 317 cases are deferred:
`curve.curve_p2` (`SmCurve::IsKindOf` in `IsDegenerate` / `HasRepeatedEndControlPoints`,
`EvaluateContinuity` in `CreateSimpleOffset`), `curve.curve_p4` (`SmCurve::IsClosed` in
`GetMaxAnalyticDomain`, `SmCurve::IsDegenerate`), `curve.curve_p5` (`SmCurve::IsLine` for
rational / non-evenly-spaced curves, `SmCurve::IsArc`) and `curve.line` (`SmLine` built by
`CreateAnalyticCurve`). Wiring its two `UNPORTED` stubs dropped `nurbs.b_spline_curve_n_lib`
from 150 to 57 deferred cases (all `curve.curve_p2` now). Upstream behaviour worth knowing:

- **`USE_ANALYTICS` is on.** `SmConfig.h` defines it, so `CreateAnalyticCurve` really builds
  `SmLine` / `SmCircle` objects (it crashes on a curve without NURB at its first
  `EvaluatePoint`). The port returns `Unported` where they would be built; `AnalyticCurve`
  is an empty enum until `curve.line` / `curve.ellipse_p1` land.
- `CalculateBoundingBox` picks its pseudo-box basis from control points `ii*lInc` counted
  from the start of the whole polygon, not from the first point of the spans covering the
  interval (`lStartPoint`); kept. Its Bezier special case calls the virtual `IsLine` on the
  whole curve and evaluates the line at the *unclamped* interval ends.
- `SetCanonical` swaps in the new NURB and dimension but keeps the old knot type and form.
- `GetControlPointsExpert` writes two values per point even for a curve of dimension 1
  (past a stride of 1); the ffi rejects that, the port returns `InvalidInput`.
- The `.smc` files in `pt_TestFiles` are `ReadFromDB` ASCII streams of version <= 32
  (no knot type / form lines); a non-zero rational flag (not only 1) reads weights, and
  the curve takes the file's dimension (`ConvertTo2D` only for a 3D file read as 2D).
- Harness note: `serde_json` (without `float_roundtrip`) can parse a fixture float one ulp
  off the correctly rounded value `read_smc` gets from the same text, so fixture intervals
  meant to equal a curve's last knot may land an ulp outside; the ffi's interval checks for
  `BBox` allow a few ulps (upstream clamps them).
