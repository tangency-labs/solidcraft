# nlib.crv_basic (partial; no disagreements)

Difftest: 3417 cases (417 fixture + 3000 generated), 3139 bit-identical (max_rel_err 0),
278 deferred; a 30,000-generated-case run is also clean. Fixtures use the six upstream
`.smc` curves, the `.sms` surfaces (cross-boundary derivatives), the rectangle / polygon
segments of `TestSmCreateRectangle` / `TestSmCreateRegularPolygon` (including the far-centre
degenerate ones), the 16 line pairs of `my_test_reversed_line_join`, conic data and edge
cases.

- **Deferred: `N_CrvsAreCoincident`** calls `N_CrvsMakeCompatible` (unit
  `nlib.crv_tool_p2`, back-edge). The Rust side returns `Unported` (stub
  `UNPORTED(nlib.crv_tool_p2)` in `nurbs/crv_basic.rs`); every `coincident` case is
  deferred until that unit lands. Upstream turns that call's failure into `false`; the port
  propagates the error. The FFI only passes well-formed clamped B-splines to it.
- **Uninitialised output in `N_CrvGetNumAndDenom`.** With a rational curve, a NULL numerator
  and a too-small denominator, upstream allocates the numerator (`malloc`), then fails
  (`NL_STO_ERR`) before writing it: the numerator holds garbage (Rust: zeros). The generator
  always passes an existing numerator in that situation.
- **`N_CrvGetConicData` loops forever** for non-finite angles or angles where `ae + 360 == ae`;
  the port returns `InvalidInput` for non-finite angles and after 2^20 turns. An unknown
  conic flag leaves `n`, `p` unwritten upstream (not expressible with `ConicType`).
- **`N_CrvIsPlanar`** reads `ang` from `N_VectorsAngle` even when that call failed (stale or
  uninitialised value) if the cross product still normalises; the port keeps the previous
  vertex's angle (NaN at the first). It also writes `PP`/`NN` (zeroed `NN`) on its error
  path; the port returns only the error and the difftest does not compare them then.
- **`N_CrvReparamToInterval`** writes `U[m-p..]` for `m < p` (negative index): `InvalidInput`.
- **`N_CrvSizeArrays` compares with the indices in use**, not the storage (as
  `N_CFuncSizeArrays`); the port is faithful (unlike `nlib.srf_basic`'s `size_arrays`).
- **`N_CrossBoundDerivCrvNonRatSrf`** reads `Pw[1]` / `Pw[n-1]` and `U[p+1]` / `U[n+p]` (v
  likewise) without checks; too-small surfaces are `InvalidInput` and refused by the FFI.
- No `UNPORTED(nlib.crv_basic)` stubs existed to wire.
