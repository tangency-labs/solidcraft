# nlib.crv_fit_p1 (green; no disagreements)

2084 difftest cases (84 fixtures from `FitCrvPoints.pts`, `Points.pts`, rectangle / hexagon
corners of `TestSmCreateRectangle` / `TestSmCreateRegularPolygon`, and edge cases; 2000
generated), all bit-identical (`max_rel_err = 0`).

- **`N_FitCrvFirstDerivAndKnots` writes `M[2i+1][-1]`** whenever the span of `u[i]` gives
  `l == 0` (not with the knots of its driver `N_FitCalcKnotsDerivs`, where `l == 1`). The
  banded matrix is one row-major block, so that is the last entry of row `2i` (band column
  `bw-1`, i.e. a wrong matrix entry). The port writes exactly there; fixture
  `first_derivs_row_underflow_p2` matches C++ bit for bit through it.
- **Unwritten outputs.** `N_FitCalcCrvParamValues` leaves the interior parameters unwritten
  when the total chord length is `<= NL_PTOL`, and `N_FitCalcKnotVectorDeriv` leaves the
  interior knots unwritten for `p == 0`; the port returns zeros there and the shim passes
  zero-filled buffers.
- **Out-of-bounds reads/writes refused by the FFI and `InvalidInput` in Rust:**
  `N_FitCalcKnotVectorEndDerivs` / `N_FitCalcKnotVectorDeriv` with `p == 1` and one
  parameter read `u[1]` / `u[-1]`; `N_FitCrvApproxKnots` copies `n+p+2` knots before checking
  the knot count, writes `Pw[1]` for `n == 0`, and its band bookkeeping (`start` / `end` /
  `index`) indexes `B` correctly only when the evaluated parameters `u[1..m-1]` are
  non-decreasing; `N_FitCrvCPtsFromSrfData` with a matrix larger than the extracted row /
  column reads uninitialised entries; `N_FitCrvInterpGivenParams` with `2 <= n < p` gives
  `N_BasisEval` too few knots.
- `N_FitCrvKnotsAndDeriv` uses band offset `l = 0` for every row when `p == 2` and
  `l = j - i - 1` otherwise (not `sp - i + j` as its two-derivative sibling); ported as is.
- No `UNPORTED(nlib.crv_fit_p1)` stubs existed to wire.
