# nlib.funcs_basic (green; no disagreements)

Difftest: 3130 cases (130 fixture + 3000 generated), bit-identical (max_rel_err 0); a
50,130-case run is also clean. Fixtures evaluate the control-point coordinates of the
upstream `.smc` / `.sms` test files as scalar curve / surface functions, plus edge cases.

- **Unchecked indices.** `N_CFuncSetSizeIndices` / `N_SFuncSetSizeIndices` and
  `N_CrvFuncSetPtrs` trust the caller; indices past the storage make later reads go out of
  bounds. The evaluators (`N_CFuncEval`, `N_CFuncDerivs`, `N_SrfFuncEvalPt`) read the value
  *storage* at `spn-p..=spn` with `spn <= m-p-1` and never compare with `n` (so a value array
  shorter than `m - p` entries is read past its end, while a larger storage is read beyond
  the indices in use). The Rust port reads the storage as well (same results) and returns
  `InvalidInput` instead of reading past it; the FFI wrapper refuses such inputs.
- **`N_CFuncSizeArrays` / `N_SFuncSizeArrays` compare with the indices in use**, not the
  storage: an existing function whose indices were shrunk cannot be regrown within its own
  storage (`NL_STO_ERR`). Kept (note `nlib.srf_basic`'s `size_arrays` reallocates instead,
  following `N_SrfSizeArrays`).
- A function is "NULL" (`N_CrvAreFuncArraysNULL`) when any pointer is NULL **or** the degree
  is `-1`; Rust uses `Option<CurveFunction>` / `Option<SurfaceFunction>`, so partly NULL
  functions cannot be expressed. Storage upstream allocates with `malloc` is zero-filled.
- No `UNPORTED(nlib.funcs_basic)` stubs existed to wire.

### Allocation-safety follow-up (2026-10-17)

`CurveFunction::size_arrays(&mut None, usize::MAX, 1, 0)` panicked with
`capacity overflow`: the port used `vec![0.0; n.saturating_add(1)]` for new
storage. `SurfaceFunction::size_arrays` had the same issue for all four storage
indices, plus infallible cloning of the rows in its nested `vec!` expression.

New storage now checks each index-to-length conversion and byte layout before
allocating, including the complete surface grid's checked row/column product.
Each allocation uses `try_reserve_exact` before zero-filling; failures return
`SolidError::Nlib { code: NlibCode::Memory, .. }` at the sizing operation. The
slot is inserted only after all allocations succeed. Existing-slot sizing
retains the upstream `NL_STO_ERR` behavior and leaves the function unchanged
on failure. This follows the memory-error path of `N_AllocReal1dArray` and
`N_AllocReal2dArray` in `source/SMLib/src/NL_Util.cpp`.

Rust regressions cover `usize::MAX` in every storage dimension, per-array byte
layout overflow, whole-grid byte/count overflow, and existing-slot preservation.
These unrepresentable sizes are Rust-only safety tests: they are not sent through
the C++ shim or claimed as differential passes. The existing differential fixtures
exercise normal allocation, resizing, accessors and evaluation; no FFI guard or
numerical tolerance was changed. This handles representability/reservation failures;
it does not impose an application memory quota or guarantee recovery from an OS
out-of-memory kill after a successful reservation.
