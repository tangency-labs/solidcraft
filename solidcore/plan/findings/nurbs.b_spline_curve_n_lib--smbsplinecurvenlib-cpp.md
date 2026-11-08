# nurbs.b_spline_curve_n_lib (`SmBSplineCurveNLib.cpp`)

No Rust-vs-C++ disagreement: 2132 of 2132 non-deferred difftest cases bit-identical (282
fixtures on the six `pt_TestFiles` `.smc` curves, `FitCrvPoints.pts` / `Points.pts`,
`my_test_curve_interface`, the `smSurfaceTest.cpp:1975` helix, `TestSmMakeCurvesCompatible`,
the `TestSmCreateRectangle` / `TestSmCreateRegularPolygon` edges and edge cases, plus 2000
generated); a 100,282-case run was also clean. 150 cases are deferred on
`nurbs.b_spline_curve_p2`: every `SetIntersectionCurveThroughPoint` (it starts with
`IsIntersectorApprox` = `GetDegree() == 3 && IsPiecewiseC0()`) and `DeformToEndPointTargets`
of curves of degree below 3 or of even degree (`DegreeElevate`). No
`UNPORTED(nurbs.b_spline_curve_n_lib)` stubs existed to wire. Upstream behaviour worth knowing:

- **`DampenKnotSpacing` can loop forever.** Each pass halves every span more than
  `dMaximumDeltaFactor` times longer than a neighbour; for factors below about 2 the halving
  cascades without end on many knot vectors (e.g. unique knots `0 .3 .6 1` with factor 1.2),
  and a factor at most 1 never finishes. The port stops after `DAMPEN_SPLIT_LIMIT` splits with
  `NotConverged`; the ffi wrapper simulates the splitting and rejects inputs that do not
  finish within 4000 splits.
- **`CreateHelixSegment` passes inputs that crash `N_CrvApproxSpiral`.** Its checks allow a
  height in `[-SM_EFF_ZERO, NL_MTOL)`, radii (both) below `NL_MTOL` and turns in
  `[SM_EFF_ZERO, NL_MTOL)`, which hit the spiral's early input errors (the uninitialised
  `N_EndNurbs(&SL)` crash of `nlib.spiral`); the ffi rejects them, the port returns `SM_ERR`.
- **`Trim` and `sm_CrvTrim`.** `sm_CrvTrim` returns `NL_INP_ERR` (not `NL_YES`) for
  `ur <= ul`, so `Trim` would go on with a `NULL` curve; the snapping and degenerate-interval
  checks before it make that unreachable for a proper interval. The port returns
  `InvalidInput`. `SmTol::GetZoneTol3d(this)` / `SmTol::MapTo1d(tol, t, *this)` (owned by
  `geom.tol`, stubbed there for `SmCurve`) are computed locally: a curve without edge has the
  system zone tolerance, and `MapTo1d` evaluates through `SmCurve`'s static type, i.e.
  `Evaluate(t, 1, TRUE, .., bNonZeroTangents = TRUE)`.
- `CreateInterpolatingCurve` reinterprets its `const` vector array as the in/out `D` of
  `N_FitCrvFirstDeriv`, so with `bIgnoreVectorMagnitude` the caller's vectors come back
  normalised and rescaled. The port works on a copy.
- `InterpolatePoints` copies `cpOptParams->GetSize()` parameters into an array of `m + 1`
  (reading unset memory or writing past it when the sizes differ); the port requires one
  parameter per point.
- `SetIntersectionCurveThroughPoint` appends the UV control point spacings once per
  through point (2 or 3 per UV curve) but reads them by UV curve index, so UV curve `i > 0`
  uses curve 0's spacing; ported as is (deferred until `IsPiecewiseC0` lands).
- `DeformToEndPointTargets` ignores the status of its in-place knot insertions and of the
  Reg2 refinement (an empty Reg2 knot vector fails `N_CrvRefine`, which is harmless: the
  displacement curve is then already as large as the curve); the port keeps the unchanged
  curve on such a failure.
