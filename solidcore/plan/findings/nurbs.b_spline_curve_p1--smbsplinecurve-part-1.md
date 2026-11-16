# nurbs.b_spline_curve_p1 (SmBSplineCurve part 1)

No Rust-vs-C++ disagreement: 2156 of 2156 non-deferred difftest cases bit-identical (216
fixtures on the six `pt_TestFiles` `.smc` curves, the `smSweepTest.cpp` lines and circles
and edge cases, plus 2000 generated; a 30,216-case run was also clean). 60 cases are
deferred on `curve.curve_p2` (`SmCurve::EvaluateContinuity` for curves with interior knots,
`SmCurve::IsKindOf` for other types, `ApproximateLength`) and `curve.curve_p1`
(`TessellateByBisection`). Upstream behaviour worth knowing:

- `SmBSplineCurve.h` declares `Evaluate(..., bNonZeroTangents = FALSE)` while `SmCurve.h`
  says `TRUE`. Default arguments bind by static type, so the recursion inside
  `SmBSplineCurve::Evaluate` for a zero first derivative with one derivative asked
  (`Evaluate(dParameter, 2, bFromLeft, sVals)`) runs with `FALSE` and hands back the exact
  zero derivative: the "non-zero tangent" fix only acts when 2+ derivatives are asked.
  The port passes `false` there (caught by the `zero_tangent_start_nd1` fixture).
- Upstream dereferences the `NULL` NURB of a curve built with `SmBSplineCurve(lDim)` in
  nearly every query (`GetOrCreateGwNurbPointer` asserts only in debug builds); the port
  returns `SM_ERR` and the ffi wrapper only runs the NULL-safe ops on such curves.
- `RemoveExtraKnots` on a borrowed NURB swaps the shrunken copy in but keeps
  `m_bNurbIsBorrowed = TRUE`, and its temporary frees the borrowed curve; the port keeps the
  flag as upstream (ownership is moot in Rust).
