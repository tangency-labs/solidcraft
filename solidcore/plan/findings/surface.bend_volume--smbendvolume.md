# surface.bend_volume (`SmBendVolume`)

All non-deferred difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 2347
default cases, a 30,847-case run clean, `--perturb` SENSITIVE). The 75 deferred cases are
`IsCurveInParamDomainSimple` (74: `SmCurve::IsOnPositiveSideOfPlanes`, unit
`curve.curve_p9`, open in PR #13) and `ReadFromDB` (1: base read `SmVolume::ReadFromDB`, unit
`surface.volume_p2`). No upstream test builds a bend volume. Upstream behaviour worth knowing
(all ported as written):
- **`EvaluateBoundingBoxSimple` is analytic and has slips.** With `u` reaching
  `SM_INFINITE_PARAMETER`, the bounded `y` maximum is taken from the *cosines* of the end
  angles, and the `y` minimum is unbounded whenever `dThetaMin < 0 || dThetaMax <= 0`; the
  `SM_IS_CONTAINED` angle tests are not reduced to one period. The result is set with
  `SetMinMax`, whose failure for an inverted box (status ignored) leaves the caller's box as
  it was (a fresh `SmExtent3d` stays in its `Init` state).
- **`TrimParamBoundingBoxSimple`** clamps each `v` end to the primary period separately, so a
  box crossing `v = pi d` wraps to an inverted box; `SetMinMax` then fails silently and the
  output box keeps its previous contents (status `SM_SUCCESS`). It `Init`s the output and
  returns `SM_ERR` when the box is entirely below `u = singularity_tol` or outside the period.
- **`InvEvaluateGuessPointSimple`** returns `SM_ERR` within `GetLegalMinU()` of the axis but
  still writes its guess (`theta = 0`); on the seam (`theta = +-pi` within the scaled zero) it
  writes two guesses. `GlobalPointSolveSimple` ignores the parameter domain and never fails.
- **`IsBoundedSimple` is `TRUE`** (the half space and one period), so the base `IsBounded`
  is `TRUE` for every compound volume containing a bend, although the natural domain
  `[tol, inf] x [-pi d, pi d] x [-inf, inf]` is unbounded in `u` and `w`.
- **Constructor repair** has the same `Unitize` status comparison as the twist and unbend
  volumes (only the last zero-vector branch runs); a neutral distance below the clamped
  singularity tolerance becomes `1.0`. The default constructor gives neutral distance `0` and
  tolerance `SM_UNDEF_DOUBLE`, so its evaluations divide by zero (NaN / infinite values on
  both sides). `GW_MAX_DERIV` is not used: `Evaluate` / `EvaluateSimple` reject `n > 3`.
