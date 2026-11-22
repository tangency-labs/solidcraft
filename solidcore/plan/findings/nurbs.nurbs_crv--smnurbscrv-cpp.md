# nurbs.nurbs_crv (`SmNurbsCrv.cpp`)

No Rust-vs-C++ disagreement: all 4089 difftest cases (1089 fixture + 3000 generated)
are bit-identical, 0 deferred; a 51,089-case run was also clean. Upstream behaviour
worth knowing:

- `sm_BasisEval` starts its flag at `NL_YES` for early exits, so at `u == U[p]` and
  `u == U[m-p]` it fills `N` but returns the error flag; `sm_CrvEval` therefore fails
  at both domain ends. Its only caller (`SmBSplineCurve::EvaluatePoint`) evaluates the
  end control points itself there. The port returns `Unsupported` at the ends.
- `sm_CrvTrim` returns the flag value `NL_INP_ERR` (14), not `NL_YES`, for `ur <= ul`,
  without `N_ErrSet`; callers comparing with `NL_YES` treat it as success.
- `sm_CrvDerivs` rejects `der > GW_MAX_DERIV` (5) but `der == 5` writes past its
  `[GW_MAX_DERIV]` row arrays; the port rejects `der >= 5`.
- `sm_CrvRemoveKnots` can return a stale `NL_YES` from a failed (and recovered)
  `sm_BasisFindAllSpanMaxima` when no knot is removed; the port returns that flag as
  `Ok(bool)`. Its rational weight check `continue`s after already updating the error
  vector `er`.
- `sm_CreateBezSegments` keeps the "next segment" pointer from the previous iteration
  for the last segment (and NULL for a single span); writes through it happen only
  for knot vectors with trailing over-multiplicity.
- The `sm_Basis*` index/convergence errors go through `smos_ErrorMessage`, which does
  not set `NL_ERROR.eno`; the difftest compares returned flags only.
