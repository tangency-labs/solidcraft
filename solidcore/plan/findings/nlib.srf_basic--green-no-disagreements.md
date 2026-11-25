# nlib.srf_basic (green; no disagreements)

3220 default cases (220 fixture + 3000 generated) and a 100,220-case run (`--seeds
100000`) agree bit for bit with upstream (`max_rel_err = 0`). Fixtures include the
upstream surfaces `Surface.sms` (thinly closed in u), `offset_nurb.sms` and
`DropCurveSurface.sms`, read by the runner. Notes for later agents:

- **`nurbs::srf_basic::Surface` is the `NL_SURFACE`.** It owns its net and knots but, as
  upstream, keeps the indices in use (`n`, `m`, `r`, `s`) apart from the storage, which may
  be larger (`Surface::with_indices`, `set_size_indices`). The NULL surface of
  `N_SrfInitArrays` is `None` in an `Option<Surface>`; `size_arrays` / `copy_into` take
  that `Option` (`N_SrfSizeArrays` / `N_SrfCopy`). Storage upstream `malloc`s is
  zero-filled. `KnotVector`s are borrowed from the surface with `knot_vectors()`.
- **`N_SrfIsSized` compares the indices in use, not the capacity.** So `N_SrfSizeArrays`
  reallocates whenever a request exceeds the current `n`/`m`/`r`/`s`, even if the storage
  is big enough; spare capacity is only reachable through `N_SrfSetSizeIndices`. When it
  reallocates, `N_SrfSizeArrays` returns 0 but leaves `NL_STO_ERR` in `NL_ERROR` (set by
  `N_SrfIsSized`); the Rust port records nothing.
- **Weight checks.** `N_SrfWeightsAreValid` lets NaN weights through (`<`/`>` tests); a
  non-rational point (`w = NL_NOW`) in a rational surface fails it as a weight error.
  Rationality is decided by `Pw[0][0]` alone (`N_IsSrfRat`).
- **Undefined upstream paths, made errors in Rust.** `N_SrfReparamToInterval` writes
  `U[r-p..]` outside the array when `r < p` (`InvalidInput`, nothing changed);
  `N_SrfSetSizeIndices` / `N_SrfSetCPtsAndKnots` accept indices past the storage
  (`InvalidInput`). The ffi wrapper never passes those inputs.
- **Unwritten outputs.** `N_SrfIsFlatCheap` leaves `flt`/`dir` unset when
  `N_PlanePtNormalToImplicit` fails (the normal's `1/|N|` overflows); the port returns only
  the error. Its closed and degenerate exits use the global `NL_MTOL`
  (`Surface::is_closed_tol` takes it explicitly for `N_SrfIsClosedSmooth`).
- **`N_SrfIsClosed` with `NL_UVDIR`** (or any flag but `NL_UDIR`/`NL_VDIR`) is `false`.
- No `UNPORTED(nlib.srf_basic)` stubs existed to wire. `nlib.math`'s power-basis
  routines still take the net directly; a `Surface` method may forward to them.
