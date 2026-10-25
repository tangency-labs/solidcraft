# nlib.srf_common (`NL_SrfCommon.cpp`)

No Rust-vs-C++ disagreement: 1550 of 1550 difftest cases bit-identical (50 fixtures from
`TestSmCreateSurfaceFromCornerPoints`, `TestSmCreateExtrude`, `TestSmCreateRuledSurface`,
`TestSmCreateSurfaceRevolution`, `my_test_surf_methods` and the `.smc` curves in
`pt_TestFiles`, plus 1500 generated). No `UNPORTED(nlib.srf_common)` stubs existed.
Upstream behaviour worth knowing:

- `N_CreateRevolvedSrf` stores the circle weights from `N_CalcCircWeights` in a local
  `NL_REAL wi[9]`. A quartic (`NL_QUARTIC`) revolution of 241 degrees or more has 13
  weights and overflows the stack buffer (undefined behaviour). Upstream callers only pass
  `NL_QUADRATIC` or a caller-chosen `rtp` (`NL_SrfFit.cpp:13242`, `:13828`). The port keeps
  all the weights; the ffi wrapper rejects those inputs so difftest never runs the overflow.
- The routines change their input curves in place: `N_CreateSrfExtrudeCrv` makes a 2D
  curve 3D (after the `dir` check, before the direction is normalised, so a zero direction
  still converts the curve), `N_CreateRuledSrf` makes both curves compatible (and keeps the
  changes made before a failure), `N_CreateRevolvedSrf` makes the profile rational and 3D.
  The port takes `&mut Curve`; difftest compares the curves after the call.
- `N_CreateCylCone` reads the indices and knots of the top circle when there is one, else
  of the bottom circle; a collapsed circle (radius at most `NL_MTOL`) becomes its centre
  weighted by the other circle's weights. `N_CreateRuledSrf` takes `n` from curve B and
  `p`, `m`, `U` from curve A (equal after `N_CrvsMakeCompatible`).
