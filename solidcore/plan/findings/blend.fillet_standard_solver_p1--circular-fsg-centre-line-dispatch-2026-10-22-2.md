# blend.fillet_standard_solver_p1: circular FSG centre line dispatch (2026-10-22)

- `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` now takes the centre line as a
  `Curve` of an `SmBSplineCurve` kind, so `GetDegree` / `IsLine` / `IsArc` / `Evaluate`
  dispatch on its kind (the earlier note above, "not hit by a compared case yet", is
  resolved). It became reachable with #307 (concrete surfaces in `CrvOnSurf`): the
  generated `offset.offset_geometry_creation` case `gen0639` (`edgeuse_offset` on
  `plane_square.smb`) builds its centre line from an `SmLine` edge curve whose NURB
  `MakeCurvesCompatible` raises to degree 3. Upstream `SmLine::GetDegree` still
  answers 1, takes the sweep test, and `SmVector3d::AngleBetween` of the rail line
  (`(-10, 0, 0)`) against the zero `SmLine::IsLine` vector (`(0, -0, 0)`)
  returns `SM_ERR_INVALID_INPUT` (1007; both located with gdb on the reference build). The B-spline-data port skipped the
  sweep test and stopped at the zero-radius cross section (`SM_ERR`, 1001):
  `values.state[410]` C++ 1007 vs Rust 1001. With the dispatch both give 1007.
