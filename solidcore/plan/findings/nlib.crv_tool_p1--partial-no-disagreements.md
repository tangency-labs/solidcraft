# nlib.crv_tool_p1 (partial; no disagreements)

2143 default cases (143 fixtures on the `.smc` test curves, 2000 generated): 2063 agree bit
for bit (`max_rel_err = 0`), 80 deferred. C++ runs both the in-place and out-of-place forms;
the Rust methods return a new curve. Upstream behaviour worth knowing:
- **Deferred:** `N_CrvRemoveKnot` on a rational curve calls `N_CrvGetMinMaxWeightsAndPts`
  (`nlib.crv_nurb`). That routine always returns `pmax = 0`, so the scaled tolerance is
  `tol * wmin`. Wire `UNPORTED(nlib.crv_nurb)` in `nurbs/crv_tool/p1.rs`; the rational
  weight check after it is already ported.
- **`N_CrvRemoveKnot` error flag:** a failing `N_BasisFindGlobalMax` gives an assumed maximum
  of 1, but the flag stays in `error`. If the last such call failed, the routine finishes the
  removal and still returns 1. The port returns only the error then; the shim dumps nothing
  on a nonzero status.
- **No input checks:** `N_CrvInsertKnot`, `N_CrvSplit`, `N_CrvDecomposeBez`, `N_CrvRefine`,
  `N_CrvExtractCrvSeg` and `N_CrvRemoveKnotMaxErr` don't validate the curve. For unclamped
  knots, multiplicity above `p`, out-of-range `ul`/`ur` or wrong `(r, s)` they index outside
  their arrays. The port returns `InvalidInput`, and the FFI refuses such inputs.
  `N_CrvDecomposeBez` past `nsp` segments does `NL_QUIT` with no error number.
- **Fixture parameters:** serde_json (without `float_roundtrip`) can parse decimal text
  1 ulp off, so knot-valued parameters in fixtures use `{"knot": i}`. Without it, the
  `curve0.smc` end knots fell outside the range.
- No `UNPORTED(nlib.crv_tool_p1)` stubs existed to wire.
