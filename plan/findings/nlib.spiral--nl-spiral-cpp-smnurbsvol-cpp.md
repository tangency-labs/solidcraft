# nlib.spiral (`NL_Spiral.cpp`, `SmNurbsVol.cpp`)

No Rust-vs-C++ disagreement: 2032 of 2032 difftest cases bit-identical (32 fixtures from
`TestSmCreateHelixInvalidInputs`, `smSurfaceTest.cpp:1975` and edge cases, plus 2000
generated; a 20k-seed run is clean). No `UNPORTED(nlib.spiral)` stubs existed.
Upstream behaviour worth knowing:

- `N_CrvApproxSpiral` crashes on its own input errors: the checks `height < NL_MTOL`,
  a radius `< -NL_MTOL`, `rad0 + rad1 < NL_MTOL` (after zeroing radii below `NL_MTOL`)
  and `NumTurns < NL_MTOL` run `NL_ERROR` (goto `EXIT`) *before*
  `N_InitNurbs(&SL)`, and `EXIT` calls `N_EndNurbs(&SL)` on the uninitialised local
  stacks (`free(): invalid pointer`, e.g. height 0 with r0 = r1 = 2, turns 5). The
  public callers (`SmApiCreateHelix`, `SmBSplineCurve::CreateHelixSegment`) validate
  first, so they never reach it. The port returns `NlibCode::Input`; the ffi rejects
  these inputs (`hits_early_input_error`) and the Rust unit tests cover them.
- Fewer than 1/8 turn when fitting (`NumPoints <= 1`): the initial span count
  `(int)(8 * NumTurns + 0.99)` is 0, `dTheta` divides by zero, and the cubic spline
  fit with one point fails with `NL_INP_ERR` (after `N_InitNurbs`, so no crash).
- When the span count reaches `NL_CCPLIM - 6` (994) without meeting `tol`, the loop
  adds the next increment and stops, so `*NumPoints` comes back as
  `994 + delta + 1` while the curve was fitted with 995 points. Ported as is.
- The span count conversions are C `(int)` casts of doubles (undefined beyond the `int`
  range; Rust saturates); the ffi caps turns at 1000 and `NumPoints` at 100000.
