# nlib.srf_adv_p4 (NL_SrfAdv.cpp part 4: Coons surfaces, cross-boundary derivatives)

No Rust-vs-C++ disagreement: 3155 of 3155 difftest cases bit-identical (155 fixtures on the
six `pt_TestFiles` `.smc` curves plus Coons, knot-merge, weight-scale and derivative-strip
edge cases, and 3000 generated; a 60,155-case run was also clean). Upstream behaviour worth
knowing (all kept out of the generated inputs, documented in the module header):

- `N_ApproxCrossBoundaryDerivs` ignores the status of `N_VectorsAngle`: a zero second or
  next-to-last control point leaves the end angle uninitialised. When one end angle equals
  `eps` exactly while the other exceeds it (e.g. `eps = 0` with an exactly aligned end),
  neither knot branch runs and `N_CrvRefine` inserts the uninitialised `UX[0]` (malloc'd,
  not zeroed); upstream then "succeeds" with garbage. The port returns the angle error /
  `InvalidInput` in these cases.
- `N_SweepCrossTangentCrv` accumulates sample parameters (`params[i] = params[i-1] + du`),
  so the last one can round past the last knot; it ignores the `N_CrvEval` failure and
  reuses the previous sample. The port does the same (only a failure on the very first
  sample, where upstream reads an uninitialised point, is returned).
- `N_MergeKnotVectors` loops forever on knots that compare unordered (NaN); the port
  returns `InvalidInput`, the ffi wrapper rejects non-finite input.
- `N_CreateCoonsSrf`, `N_CreateCoonsBoundaryCrvs`, `N_CreateSkinSrfBoundaryDerivs` and
  `N_ApproxCrossBoundaryDerivs` modify their input curves (rationalised, reparametrised,
  made compatible, made 3D, mid knot inserted); difftest compares the curves after the call.
