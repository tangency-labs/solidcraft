# curve.tangent_field

- **Base class not ported.** `SmTangentField` derives from `SmCrvOnSurf`
  (`curve.crv_on_surf`). The port keeps the UV curve and surface as borrowed references
  (upstream's owner flag `0`) and co-ports `SetAll` (given UV domain only),
  `GetNaturalInterval`, `IsKindOf` and `operator==` (`// CO-PORT(curve.crv_on_surf)`).
  Deferred: every constructor without a UV domain (`SmSurface::GetNaturalUVDomain`,
  `surface.surface_p2`) - which includes every successful `Create`, since upstream always
  passes `NULL`; every evaluation inside the end bands (`SmSurface::EvaluateNormal`,
  `surface.surface_p7`); equality of fields on different surface objects
  (`SmSurface::operator==`, `surface.surface_p1`); and every `ReadFromDB` that reads the
  members (`SmCrvOnSurf::ReadFromDB`).
- `EvaluatePoint` returns the stored start (end) tangent for `t < min + 1e-8 + EFF_ZERO`
  (`t > max - 1e-8 - EFF_ZERO`), so `Evaluate(n = 1)` there is a zero derivative, and
  `Evaluate` with `n >= 2` is `SM_ERR` before any evaluation.
- `ReadFromDB` ignores the status of the `SmCrvOnSurf` base read and returns `SM_SUCCESS`;
  binary short reads of the members are undefined upstream (see `database_io` above), so
  difftest never truncates inside them.
