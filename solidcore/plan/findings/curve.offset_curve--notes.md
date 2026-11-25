# curve.offset_curve

Green, no disagreement (2808 cases bit-identical). Upstream behaviour worth knowing:
- `SmOffsetCurve::Evaluate` with a zero offset distance evaluates the base curve with
  `SER(...)` but does not return: the result is overwritten by the general path (same
  values with `D = 0`); only the status of that first base evaluation matters. The port
  keeps the extra call.
- With `bNonZeroTangents` and `lNumDerivatives == 1`, the recursive `Evaluate(dParameter,
  2, ...)` passes the already mirrored (and, for a zero base tangent, shifted) parameter,
  so a reversed curve is mirrored twice there; its status is ignored. Kept.
- `Trim` checks the *current* curve interval against the base curve domain, not the new
  trim interval, and never snaps it. Kept.
- `Copy` (copy constructor) of an offset that owns a base without `Copy` (a Hermite
  curve: `SmCurve::Copy` = `SM_ERR`, ignored) shares the base pointer and both curves
  delete it (double free). The port owns its base by value (a clone); the ffi wrapper
  rejects such cases.
- A zero-distance offset of a rational B-spline evaluated with more than `NL_MAXDER` (4)
  derivatives overflows a fixed NLib stack array (`sm_CrvRationalBasisDerivs`, stack
  smashing). The ffi wrapper limits `Evaluate` to 4 derivatives.
- The base curve is held by value (`Box<Curve>`), so the pointer test of `operator==`
  becomes the base curve `operator==`.
- `SmBSplineCurve::CreateSimpleOffset` now builds the `OffsetCurve`; it defers on
  `SmCurve::ApproximateCurve` (`curve.curve_p1`) instead of on this unit.
