# nlib.crv_adv_p2 (`NL_CrvAdv.cpp`, part 2)

No Rust-vs-C++ disagreement: 2815 of 2815 default difftest cases bit-identical (815
fixtures: the six upstream `.smc` curves, the `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` edges as degree-1 curves, and edge cases; 2000 generated),
and a 300,815-case run (`--seeds 300000`) also clean. `N_CrvRemoveKnotsDerivConstraints`
is `N_CrvRemoveKnots` plus a constraint check, so `crv_tool::p2::Curve::remove_knots`
now shares its body (`remove_knots_constrained`; `nlib.crv_tool_p2` re-run: 2124/2124,
report unchanged). No `UNPORTED(nlib.crv_adv_p2)` stubs existed to wire. Upstream
behaviour worth knowing:

- **`N_CrvSumDiffCrv` corrupts the heap for some rational sums.** The rational branch
  computes `num = D_q * N_p` and `tmp = D_p * N_q` with `N_CrvFuncMultiply*`, which
  rescale the knot vector of the operand with the shorter parameter range **in place**
  (so `D_p`/`D_q` change between the calls), then adds `tmp`'s control points into
  `num` for `i <= n` of `tmp`. When the two products come out with different knot
  vectors (curves over different parameter ranges, or interior knots closer than
  `NL_PTOL` but not equal) `n` of `tmp` exceeds `num`'s and upstream writes past `num`'s
  control points: the difftest died with `double free or corruption` /
  `free(): invalid next size` on such cases (or returned `NL_INP_ERR` from
  `N_CreateCrvFromNumAndDenom` when it survived). The port returns `InvalidInput` there;
  the ffi only passes rational sums over identical parameter ranges whose knots are
  equal or more than `1e-9` ranges apart. Non-rational sums over different ranges are
  fine (rescaled once by `N_GetCompatibleKnotArrayMult`) and tested.
- `N_CrvSumDiffCrv` and `N_CrvReparamRat` call `N_CrvNonRatToRat` on their **input**
  curve; `N_CrvSumDiffCrv` converts it back at the end (not on its error paths),
  `N_CrvReparamRat` leaves even an out-of-place input rational. The port leaves inputs
  alone.
- `N_CrvRatGetFirstDeriv` squares the denominator with `N_CrvFuncMultiplyCrvFunc(&d, &d)`
  (the same object twice); the port multiplies a copy (identical: equal ranges, nothing
  rescaled). Interior knots of multiplicity `p` make the squared denominator's
  multiplicity `2p`, and `N_BasisIncreaseKnotMult` then fails with `NL_KNT_ERR`.
- `N_CrvEvalUnboundedPtsAndDerivs`: the periodic shift of a G1-closed curve
  (`u = ue - us + u`) loops forever once the range is below the resolution of `u`; the
  port fails with `NotConverged`, the ffi bounds `u` and the range. Extensions run in
  place (`ST_curxcrr` with `curP == curQ`) in steps of at most `0.9` ranges, below the
  `0.9213` two-step split, so the in-place `NL_G2` free of `nlib.crv_adv_p1` cannot
  trigger. `ecur` keeps a partially extended curve after a failed step; the port keeps
  the last complete one (the shim stops at the first error).
- `N_LinearMultiplyCrv` ignores the error of `N_CrvGetGrevilleAbscissa` (degree 0) and
  then uses an uninitialised parameter; the port propagates the error.
- `N_CrvOffsetGetMax2ndDeriv` and several others allocate `N_AllocReal1dArray(mu)` and
  write index `mu`: NLib's allocators hold `hi + 1` entries, so this is in bounds.
