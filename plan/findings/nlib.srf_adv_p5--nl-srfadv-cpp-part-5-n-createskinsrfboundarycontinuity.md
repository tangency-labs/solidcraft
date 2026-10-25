# nlib.srf_adv_p5 (NL_SrfAdv.cpp part 5: N_CreateSkinSrfBoundaryContinuity)

- **Shared knot array.** In the rational case with rails, the derivative curves are built by
  `N_CrvFromCPolygonAndKnotVector` over the derivative surface's own knot vector (v for
  `NL_UDIR`, u for `NL_VDIR`), so the in-place knot writes of `N_CrvsMakeCompatibleKnotTol`
  (`N_CrvReparamToInterval`, the knot averaging of `N_GetCompatibleKnotVectorToTol`) and the
  rescaling in `N_CrvFuncMultiplyCrv4d` (`N_MakeKnotsCompatible`) also change `der[i]`'s
  knots, until a degree elevation or refinement gives the curve new storage. Only the
  in-place output `der` is affected (the surface is not). The port reproduces it with
  `crv_tool::p2::make_compatible_knot_tol_traced` (contents of each curve's entry knot
  storage); difftest compares `der` after the call bit for bit.
- With derivative surfaces and `NL_UVDIR` upstream uses an uninitialised derivative curve
  (the port returns `InvalidInput`; the FFI rejects it). With `cmp == NL_YES` upstream reads
  `Dws[j]`/`Rw` assuming the cross sections and the derivative data are compatible, and copies
  rail control points into the net assuming both rails share a knot vector; the FFI enforces
  both.
