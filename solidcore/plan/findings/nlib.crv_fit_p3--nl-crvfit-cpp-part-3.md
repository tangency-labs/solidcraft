# nlib.crv_fit_p3 (`NL_CrvFit.cpp`, part 3)

No Rust-vs-C++ disagreement: 2242 of 2242 difftest cases bit-identical (242 fixtures on
`FitCrvPoints.pts` / `Points.pts` and the call shapes of the upstream drivers, plus 2000
generated). No `UNPORTED(nlib.crv_fit_p3)` stubs existed. Upstream behaviour worth knowing:

- `N_FitCrvDerivs` / `N_FitCubicSplineInterp` estimate a zero `Ds` / `De` from a cubic
  Bezier through `P[0..=3]` / `P[k-3..=k]` whatever `k` is (out of bounds below four
  points, e.g. the `kk = 1` / `kk = 2` calls in `NL_CrvApprox.cpp` only work because they
  pass nonzero derivatives) and ignore the errors of that fit, of `N_CrvDerivs` and of the
  normalisation (a failed normalisation leaves the vector unnormalised and scales it by
  the polygon length). The port returns the fit / derivative errors (`InvalidInput`
  below four points); the ffi rejects those inputs.
- `N_FitCrvDerivs` and `N_FitCrvFirstDeriv` scale the end derivatives by `U[p+1]/p` and
  `(1 - U[n])/p`, i.e. assume the knots start at 0 and end at 1 (the comment in the
  source says "this may be where the routine gives poor results"). Ported as is.
- `N_FitCrvDerivs` with `par = NL_INHERITED` reads `u[i] = U[i+p]` from the knots
  already in the caller's curve and keeps them; with a NULL curve it reads freshly
  allocated, unset memory. The port takes the curve slot (`&mut Option<Curve>`, sized
  with `size_arrays`); a NULL slot gives zero knots.
- `N_FitCrvFirstDeriv` with `p = 2` copies only `ND[0..1][0..=1]` into each row (`hp =
  p - 1`), dropping the third basis function; ported as is.
- `N_FitCrvKnotsAndTangents` ignores the error of `N_FitCalcKnotVectorDeriv` (`p > k + 1`
  with one tangent); `N_FitCrvKnotsAndDeriv` then fails with `NL_INP_ERR` before reading
  the unset knots, which the port returns directly. Callers pass the same vector as `Ts`
  and `Te` (`NL_SrfAdv.cpp:22688`); it is then normalised and scaled twice
  (`EndTangents::Same`). The merged knot vector would have unset tail entries if a new
  knot were not below `UT[mt]` (cannot happen for increasing parameters); the port
  returns `InvalidInput` there.
