# nlib.basis_adv (green; no disagreements)

1751 default cases (251 fixture + 1500 generated) and a 100,251-case run (`--seeds 100000`)
agree bit for bit with upstream (`max_rel_err = 0`). Fixtures use the 33 knot vectors of
the `nlib.basis_basic` fixtures (every `.smc`/`.sms` in `pt_TestFiles`, `smTopoTest.cpp`,
SM_API_test rectangle/polygon layouts, doc examples) plus edge cases. Upstream behaviour
worth knowing:

- **`N_BasisDerivsArray` span walk.** Only `u[0]` and `u[n]` go through `N_BasisFindSpan`;
  the spans of `u[1..n]` are found by walking forward from the span of `u[0]` (the
  `NL_LEFT` case falls through into the `NL_RIGHT` loop, which is then a no-op). The walk
  has no bound: with `NL_LEFT` an inner parameter equal to the domain end `U[m-p]` of a
  clamped vector walks past `U[m]` (out-of-bounds read), and any inner parameter past the
  end does the same with either flag. Unsorted parameters are not rejected (spans only
  move forward). Rust returns `InvalidInput` for out-of-range reads; the ffi wrapper
  requires inner parameters `< U[m-p]` (left) / `<= U[m-p]` (right). Unlike
  `N_BasisDerivs`, there is no `1e-24` zero-denominator guard: zero knot differences give
  inf/NaN, as upstream. The `int` factor `p!/(p-k)!` overflows as in `N_BasisDerivs`
  (Rust `InvalidInput`, ffi rejects).
- **`N_BasisInsertKnots` reads `X[mx+1]`** after inserting the last new knot (the second
  comparison in the merge step). The value never changes the output, so Rust skips the
  comparison; the shim pads `X` by one slot. An empty `X` reads `X[-1]` (Rust
  `InvalidInput`). NaN knots make the merge loop forever (Rust `InvalidInput`).
- **`N_BasisIncreaseKnotMult`** fails with `NL_KNT_ERR` (4) when any inner knot would
  exceed multiplicity `p`; new knots of earlier spans are dropped (index not set).
- **`N_GetCompatibleKnotArrayMult`** has the same structure as `N_MakeKnotsCompatible`
  (same degree for both vectors, multiplicities merged to the maximum) and the same
  out-of-bounds reads past `U[m]` on unusual inputs and endless loop on NaN knots. The
  ffi rejects exactly the inputs that read past `U[m]` (issue #182 follow-up below); the
  shim still pads with `+inf`.
- No `UNPORTED(nlib.basis_adv)` stubs existed to wire.

**Issue #182, arithmetic-overflow follow-up.** The Rust API accepts `usize`
degrees and increments: `p = i64::MAX` panicked at the initial `p + 1` in
`compatible_knot_array_mult` and `increase_knot_mult`, and `t = i64::MAX`
panicked in the latter's multiplicity sum. Checked initial-index additions now
return `InvalidInput`; an overflowing nonnegative multiplicity sum exceeds the
degree and returns the existing `NL_KNT_ERR`. Three regression tests cover both
initial-index sites and both sum additions (single and repeated inner knots).
No floating-point formulas or comparison tolerances change. The separate 41
fuzzed Rust/C++ disagreements reported in #182 remain unassessed by this fix.

**Issue #182, fuzz follow-up.** On main after the overflow fix, `--fuzz 3000` gave
0 panics and 55 `differs`; `--fuzz 30000` gave 501 `differs` and 2 crashes. Two causes:

- *All differs: `compat_mult` reads past the ends (UB-13).* Rust returned its
  out-of-range `InvalidInput` while the C++ "succeeded" on the shim's `+inf` padding
  (garbage-dependent in real callers). Inputs: degree 0 on vectors whose end knots
  repeat (the fuzz `/p = 0` mutation, most cases), an inner knot moved above the end,
  an end knot replaced by a smaller value. The ffi guard now rejects exactly these:
  `compat_mult_reads_in_bounds` has the shim replay upstream's rescale (calling
  `N_BasisReparam`) and index walk, checking each `R[i]`/`S[j]` index before the read.
  The runner reports such inputs as `read_past_end` on both sides (C++ from the walk
  check, Rust from its out-of-range error), so the two detections are compared; the
  generator emits degree 0 on clamped vectors in 4 % of `compat_mult` cases.
- *Crashes: `insert` with fewer than `2p+2` knots (UB-196).* An unsorted `R` can pass
  the range check; upstream then writes `2p+2+mx+1` knots into an `mr+mx+2` buffer.
  The Rust port already returned those knots; the shim now sizes its output for them,
  so the reference runs and the values are compared.

Regression fixtures: `fixtures/nlib.basis_adv/fuzz_regressions.json`. After the fix,
`--fuzz 3000`, `30000` and `100000` give 0 differs, crashes, panics and timeouts.
