# nlib.bezier (green; no disagreements)

2223 difftest cases (223 fixture: SM_API_test rectangle/polygon segments, `FitCrvPoints.pts`,
`Points.pts`, `Surface.sms`, edge cases; 2000 generated), all bit-identical
(`max_rel_err = 0`), 0 deferred. Notes for later agents:

- **Matrices built on a freed stack.** `N_BezElevateDegree`, `N_BezSrfElevateDegree` and
  the `N_BezFuncMultiplyBez*` routines compute a NULL elevation / product matrix into the
  caller's `NL_RMATRIX` on a local `NL_STACKS` that `N_EndNurbs` frees before returning, so
  the caller is left with dangling storage. The Rust routines take `&RealMatrix` and build
  a local matrix when it `is_null()`; callers that want reuse should call
  `elevation_matrix` / `product_matrix` first. With degree `p = 0` the matrix has
  bandwidth 0 and is "NULL" again (recomputed every call), harmless.
- **`NL_PascalTri` is `[18][18]`.** Every routine using it (elevation/product matrices,
  `N_BezCrvExtend`, hence `N_BezSrfExtend4D`) reads past it for degree sums above 17;
  Rust returns `InvalidInput`. `bezier::PASCAL_TRI` / `binomial` are the table.
- **`ST_BezCalcArcLength` gives up past `NL_RECURSELIM` (20)** by returning 1 without
  setting `NL_ERROR.eno` (shim status 1000; Rust `NotConverged`). With `tol = 0` any curved
  segment hits it; collinear control points finish (`del == 0`). NaN coordinates also end
  there.
- **`N_BezReduceDegree` needs `p >= 2`**: for `p = 1` it reads the never-written
  `alf[0]` and `Qw[-1]` (Rust `InvalidInput`). `N_BezDegreeReduceCoefs` writes `alf/oma`
  only at `1..=r` and `bet/omb` at `r..=p-2` (`r = (p-1)/2`).
- **Flags.** `N_BezCrvExtend`'s `whr`/`rev` became `ExtendAt` / `bool` (its `NL_CAL_ERR` for
  other values is unreachable); `N_BezSrfExtend4D` with a direction other than U/V returns
  a surface whose net was never written (Rust `InvalidInput`); `N_BezSrfElevateDegree`
  with `NL_UVDIR` fails with `NL_CAL_ERR` after building the matrix (kept).
- **NaN parameters** pass the `[0, 1]` checks of `N_BezEvalOneBasis` / `N_BezSplit`
  (`<`/`>` tests) and propagate, as in Rust.
- No `UNPORTED(nlib.bezier)` stubs existed to wire.
