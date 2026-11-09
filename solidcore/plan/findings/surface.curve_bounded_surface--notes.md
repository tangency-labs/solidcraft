# surface.curve_bounded_surface

- **`SmCurveBoundedSurface::operator==` reads past the UV array.** The UV loop runs to the
  number of 3D boundaries; with more 3D than UV boundaries (and equal counts, flags and
  3D curves) upstream reads `m_vUVBoundaries[i]` out of bounds and crashed the difftest
  (`drop_curve_eq_other_same` before the guard). The port stops at the end of the UV
  array; the ffi rejects `EqOther` when `n3d > nuv`.
- **`SmCurveBoundedSurface::ReadFromDB`** reads the UV boundaries with the type of the last
  3D boundary (`l3DType`, `0` when there is none, so any UV boundary then fails with
  `SM_ERR`) and drops the implicit-outer flag it reads (the new surface gets `FALSE`). The
  port keeps both; the difftest covers them with `WriteToDB`-layout ASCII records.
- **Deferred cases**: the evaluation overrides call basis-surface virtuals the Rust
  `Surface` dispatch does not have yet (`surface.surface_p1`: `EvaluateSimple`,
  `EvaluateSTEP`, `EvaluateSTEPPoint`, `SmSurface::operator==`; `surface.surface_p3`:
  `EvaluateGeometric`, `EvaluateNormalSection`; `surface.surface_p5`:
  `EvaluateNormalAtSingularity`; `surface.surface_p7`: `EvaluateNormal`). `SmCrvOnSurf` and
  `SmIsoCurve` still hold a B-spline surface; reading one over a curve bounded surface
  returns `Unported` (`surface.surface_p1`).
