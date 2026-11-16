# curve.iso_curve

- `SmIsoCurve::CreateUVIsoLine` tests `m_eSurfParam == SM_SP_U` in both branches, so a
  constant-v iso curve (and every other `SmSurfParamType`) gets `SM_ERR_INVALID_INPUT`;
  the constant-u branch builds the UV line over the domain's *u* interval although the
  line runs along v. The port keeps both (difftest `*_uvline` cases).
- The empty I/O constructor `SmIsoCurve()` leaves `m_cpSurface` uninitialised; only
  `ReadFromDB` uses it, and the port always builds a new curve there.
- The base surface is held by value as a `BSplineSurface` (the only ported `SmSurface`
  class); `operator==`'s pointer-identity shortcut becomes a value comparison. When the
  `SmSurface` enum lands (units `surface.surface_*`), `IsoCurve::surface` should become
  that type.
