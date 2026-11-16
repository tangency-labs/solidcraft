# nurbs.b_spline_curve_p3 (SmBSplineCurve part 3)

No Rust-vs-C++ disagreement: 1229 of 1229 non-deferred difftest cases bit-identical (195
fixtures: the six `pt_TestFiles` `.smc` curves through every op, the `my_test_curve_interface`
`JoinWith` / `GetControlPoint` curves, `my_test_reversed_line_join` as B-spline lines,
`TestSmCreateRectangle` / `TestSmCreateRegularPolygon` style line joins and gaps, edge cases,
plus 2000 generated). 966 cases are deferred: `curve.curve_p2` (the `IsKindOf(SmLine_TYPE)` test
of `IsSegmentLine` / `IsOnPlane` / `JoinWith`, `IsDegenerate` in `HasInternalPole`,
`SmTol::IsDegenerate` in `AssertValid`, `ApproximateLength` for joins without a gap tolerance,
`EvaluateContinuity` in `SubdivideAtDiscontinuity`), `curve.curve_p4` (`SmCurve::AssertValid`),
`curve.curve_p5` (`IsLine` in `JoinWith`) and `curve.curve_p8` (`GlobalCurveIntersect` in
`TrimGap`). No `UNPORTED(nurbs.b_spline_curve_p3)` stubs existed. Upstream behaviour worth
knowing:

- `IsSegmentLine` returns an `SmBoolean` but uses `SER`, so a failed evaluation returns the
  status (e.g. 1001) as a "true" boolean; the shim reports it as a status.
- `AssertValid` test 4 (last two control points coincident) tests and reports the *start*
  pair's distance and scaled zero; kept. Its radius-of-curvature check (tests 13/14) compares
  `dMinRadius` before setting it (it is still `SM_BIG_DOUBLE`), so it never fires.
- Without `SM_DEBUG_CODE` the `SM_ASSERT_*_REPORT` macros ignore `pTestRequests`; the test
  level only matters as `SM_LEVEL_GIVEN` (skips `SmCurve::AssertValid`).
- `GetControlPoint` of a 2D curve takes the weight from the third slot; `CreateByScaleTransRot`
  then writes a `z` value into a 2D curve's `NOZ` slots through `SetControlPoint`.
- `JoinWith` hands `this` and the other curve to `CreateByJoining`, which may degree-elevate
  the other curve in place.
