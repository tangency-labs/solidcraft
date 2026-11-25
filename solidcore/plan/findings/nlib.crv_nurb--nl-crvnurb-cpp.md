# nlib.crv_nurb (`NL_CrvNurb.cpp`)

No Rust-vs-C++ disagreement: 2852 of 2871 difftest cases are bit-identical, 19 deferred
on `nlib.crv_geom` (`N_CrvArcLength`). Upstream behaviour worth knowing:

- `N_CrvGetMinMaxWeightsAndPts` always returns `pmax = 0` (deliberate, see its header);
  the removal tolerance of rational `N_CrvRemoveKnot` is therefore `tol * wmin`.
- `N_CrvReverse` has two codings: in place (swap loops over `n/2` and `m/2 - p`) and
  into a new curve (copies `p + 1` knots at each end from `UP[i]` and `UP[n+i+1]`).
  They agree only when `m = n + p + 1`; the port keeps both (`reverse` / `reversed`).
- `N_CrvGetDegenSegs` ignores the error flags of its `N_CrvEval` calls (a failed call
  leaves the point unset); the port propagates them (unreachable for clamped curves).
  For single-span curves it stops the whole scan at the first short-chord span whose
  midpoint detour exceeds `tol` (`break`, not `continue`).
- `N_CrvGetType` clears the errors of `N_CrvEval` / `N_CrvEvalCurvature` and reports
  `NL_NFREEFORM`; degree-1 polylines that are not lines are therefore always free-form
  (`N_CrvEvalFrenetFrame` fails with `NL_DEG_ERR`). In the line test `kh` is decremented
  for the last span and stays decremented. The circle normal `BB` is the unnormalised
  average osculating normal; `XX`/`YY` and the angle are written after `AA`/`BB`, so a
  failing normalisation returns an error with partial outputs.
- `N_CrvDerivs` aliases `left`, `right` and the denominator derivatives `dd` with the rows
  of its scratch array `a`; the port uses the same aliasing.
