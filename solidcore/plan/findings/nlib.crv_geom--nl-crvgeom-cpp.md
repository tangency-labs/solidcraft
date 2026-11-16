# nlib.crv_geom (`NL_CrvGeom.cpp`)

No Rust-vs-C++ disagreement: 1723 of 1723 difftest cases bit-identical (223 fixtures from
the six upstream `.smc` curves plus edge cases, 1500 generated). Wiring
`UNPORTED(nlib.crv_geom)` (`N_CrvArcLength` in `N_CrvGetDegenSegs`) made `nlib.crv_nurb`
green (2871/2871, 0 deferred). Upstream behaviour worth knowing:

- `N_CrvClosestPt` writes its best `u`/`Q` even when it returns `NL_CON_ERR`, and
  callers use them (`NL_CrvApprox.cpp` reads `Q` after a failure; in-place calls such as
  `&u[k]` keep the best value). The port exposes this as `Curve::closest_pt_search`
  (best values plus error, explicit `NL_ITLIM`); `closest_pt` is the strict form. When
  `N_CrvDerivs` fails on the first step (`u0` outside the knots: `NL_PAR_ERR`) the
  outputs stay unwritten (`best = None`).
- `N_ApproxCrvWithPolyline` subdivides forever when the tolerance is below the
  floating-point resolution of the curve (e.g. `tol = 0`, or a relative tolerance on a
  zero-length curve): a closed/degenerate segment test `dd < tolsq` never succeeds and
  coincident control points keep splitting. The port stops after
  `POLYLINE_MAX_SPLITS` splits with `NotConverged`; the ffi rejects such tolerances.
  With `tol <= 0` the last vertex is never appended (`dd > 0.01*tol` fails).
  For `p = 1` the tolerance (and its flag) is ignored and the knots `U[1..=n+1]` are
  returned as parameters.
- `N_CrvArcLength` returns its bare error flag (no `NL_ERROR` number) when the Bezier
  length recursion exceeds `NL_RECURSELIM` (e.g. `tol = 0`); the difftest maps it to
  status 1000 on both sides. A tolerance flag other than `NL_ABSOLUTE`/`NL_RELATIVE`
  fails with `NL_CAL_ERR`, but only after the line/circle shortcut.
