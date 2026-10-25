# curve.parabola (SmParabola)

Upstream behaviour worth knowing (all kept by the port unless noted):

- **Lazy NURB.** `ReverseParameterization` frees the NURB; the next
  `GetOrCreateGwNurbPointer` rebuilds it with `MakeNurb`. The port rebuilds it at once (same
  data, same NURB). A `Transform` in between differs: upstream skips the control points
  (`if (m_pNurb)`) and later rebuilds from the transformed placement, the port transforms
  the rebuilt control points; the results agree to rounding only. The difftest shim calls
  `GetOrCreateGwNurbPointer` after the reversal.
- **`ReadFromDB` ignores the `SmBSplineCurve::ReadFromDB` status** (and does not clip the
  domain to `±SM_INFINITE_PARAMETER_SQRT`, unlike the constructor and
  `AdjustSTEPInterval`). A short or missing base stream makes upstream read undefined
  values (huge `n`, `bad_alloc`); the runner only feeds complete base streams.
- `GlobalPointSolveSTEP` first runs `AdjustSTEPInterval` (it changes the curve), projects
  the point onto the plane and discards the projection. When the interval equals the
  current one and the curve has no NURB (its `MakeNurb` failed: zero focal distance,
  degenerate interval), `GlobalPointSolve` would run on a NULL NURB; the runner avoids
  that input. At merge `SmCurve::GlobalPointSolve` (`curve.curve_p1`) was ported and is
  called directly (the 206 formerly deferred `gps_step` cases now pass bit-identical).
- `MakeNurb` fails (`SM_ERR` from `smgu_LineLineClosestPoint`) when the end tangents are
  parallel or zero: a degenerate interval or a zero focal distance. The constructor ignores
  that status (the curve has no NURB); `AdjustSTEPInterval` returns it.
- `STEPInversion` reads the parameter off the `Y` coordinate only (`dY / 2 / F`); with
  classification the point must also be within `SM_EFF_ZERO * (1 + size)` of the curve.
- `Trim`, `EditParameterization` and `ConvertTo2D` are the `SmBSplineCurve` ones: they edit
  the NURB and leave the analytic domain stale.
