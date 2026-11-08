# nlib.basis_basic (green; no disagreements)

2605 default cases (1105 fixture + 1500 generated) and a 201,105-case run
(`--seeds 200000`) agree bit for bit with upstream (`max_rel_err = 0`). Fixtures use the
knot vectors of every `.smc`/`.sms` file in `pt_TestFiles`, the `smTopoTest.cpp` and
SM_API_test rectangle/polygon layouts, and the examples in the upstream doc comments.
Upstream behaviour worth knowing:

- **Endless loops upstream.** `N_BasisFindGlobalMax` loops forever when its bracketing step
  `(U[i+p+1]-U[i])/(10p)` is below half an ulp of the knots (huge knot values, tiny span).
  `N_GetCompatibleKnotVectorToTol` loops forever when an iteration matches no knot
  (`tol <= 0`, NaN knots). `N_MakeKnotsCompatible` loops forever on NaN knots. Rust returns
  `SolidError::InvalidInput` in exactly these states (the loop state is unchanged, so
  upstream would repeat it forever). The ffi wrappers reject these inputs.
- **`N_GetCompatibleKnotVectorToTol` reads `U[-1]`** when a vector's first knot is above
  the current smallest knot (the "move to average" test runs for vectors with no match).
  The read never changes the result. Rust skips the test for those vectors. The shim
  gives each vector a leading pad slot. It also writes into a buffer of `sum(m)+1` slots
  per vector, which a vector with fewer than `k-1` knots can overflow. Rust returns
  `InvalidInput` there, and the ffi wrapper requires at least `n_vec - 1` knots per vector.
- **`N_GetCompatibleKnotVectorToTol` needs a common parameter range.** Vectors with
  different end knots run out at different times. Upstream then fails with `NL_INP_ERR`
  (14), and Rust does the same. Multiplicities *are* carried over between vectors (each
  distinct knot gets the highest multiplicity it has anywhere), unlike the
  `N_GetCompatibleKnotVector` doc comment says.
- **`N_MakeKnotsCompatible` reads past `U[m]`** on degree-0 vectors whose ends differ by
  less than `NL_PTOL` (so no rescale happens). It can also read past `U[m]` with inner
  knots within `NL_PTOL` of an end, or with unclamped vectors. Its output buffers are
  caller-allocated and unbounded (UB-13). Rust returns `InvalidInput` for out-of-range
  reads. The ffi rejects exactly the inputs that read past `U[m]` (fuzz follow-up below);
  the shim still pads both vectors with `+inf` entries.
  The generator keeps to degree >= 1 for the "ends within `NL_PTOL`" mode. It also
  pushes the end knot `U[m]` of the shorter-ended vector into the other's insert list
  when the ends differ by less than `NL_PTOL`. Rust does the same, faithfully.
- **`N_BasisDerivs` int factor.** The factor `r *= (p-k)` is an `int`, and it is computed
  one step past the last row it scales. It overflows (UB) for `p >= 13` once
  `der >= 10`, for example. Rust returns `InvalidInput`, and the ffi wrapper rejects these inputs.
- **`N_BasisFindAllSpanMaxima` with degree 1 or a `p`-fold inner knot** returns fixed
  values (`max = 1` at `U[i+1]`) without searching. Degree 0 goes through the Newton
  path with `du = span/0 = inf`. Rust does the same, faithfully.

**Fuzz follow-up (issues #182, #253).** On main, `--fuzz 3000` gave 14 panics and 17
`differs`; `--fuzz 30000` gave 119 panics and 249 `differs` (no crashes or timeouts), all
in `make_compatible`. Two causes:

- *Panics: Rust start-index overflow.* `make_knots_compatible` computed `p + 1` / `q + 1`
  unchecked, so a degree of `i64::MAX` (the fuzz integer mutation; the C++ side rejects it,
  upstream's degree is a `short`) panicked. The indices are now checked and such degrees
  return `InvalidInput`; `check_index` (the `p + 1` of the basis routines) is checked too.
- *All differs: `N_MakeKnotsCompatible` reads past the ends (UB-13).* Rust returned its
  out-of-range `InvalidInput` while the C++ "succeeded" on the shim's `+inf` padding
  (garbage-dependent in real callers). Inputs: degree 0 on vectors whose end knots repeat
  (`/p = 0`, `/q = 0`, or a duplicated end knot), an inner knot moved above the end knot
  (also after the rescale), and vectors with too few end knots for the degree. The ffi
  guard now rejects exactly these: `make_compatible_reads_in_bounds` has the shim replay
  upstream's rescale (calling `N_BasisReparam`) and index walk, checking each
  `R[i]`/`S[j]` index before the read. The runner reports such inputs as `read_past_end`
  on both sides (C++ from the walk check, Rust from its out-of-range error), so the two
  detections are compared; the generator emits degree 0 on clamped vectors in 4 % of
  `make_compatible` cases. No Rust port bug and no harness asymmetry was found.

Regression fixtures: `fixtures/nlib.basis_basic/fuzz_regressions.json` (5 past-end
cases, 4 in-bounds neighbours that compare values). After the fix, `--fuzz 3000` and
`--fuzz 30000` give 0 differs, crashes, panics and timeouts.
