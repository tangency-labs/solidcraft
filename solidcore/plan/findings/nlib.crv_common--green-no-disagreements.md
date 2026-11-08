# nlib.crv_common (green; no disagreements)

- Rust and C++ agree bit for bit on all 2137 cases (no disagreements).
- The thin wrappers `N_CreateQuadraticArc`, `N_CreateQuarticArc`, `N_CreateQuinticArc`,
  `N_CreateEllipticalArc`, `N_CreateEllipticalArcAngType` (all `at = 0.01`, `atp = 0`
  for the first ellipse form) are not owned by any unit in `harness/callgraph/units.json`.
  Callers (e.g. `NL_CrvApprox.cpp`, `NL_SrfCommon.cpp`, `NL_Iges.cpp`,
  `SmBSplineSurfaceNLib.cpp`) should call the `*_tol` functions of
  `nurbs::crv_common` with `DEFAULT_ANGLE_TOL` (and `AngleType::Circle`).
- `N_CreateQuadraticArcTol` reads `N_IsectLineLine`'s uninitialised `ints` flag when
  that call fails (zero tangent / bad division); the port fails with `NL_INP_ERR`.
- `N_CreateEllipticalArcAngTypeTol` loops forever on a non-finite end angle (and
  `N_CrvGetConicData` on huge angles); the port returns `InvalidInput` and the ffi
  rejects non-finite inputs, angles above 1e6 degrees and zero ellipse radii (0/0 in
  the polar angle).
