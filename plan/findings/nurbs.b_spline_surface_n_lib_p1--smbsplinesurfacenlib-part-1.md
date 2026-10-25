# nurbs.b_spline_surface_n_lib_p1 (SmBSplineSurfaceNLib part 1)

No Rust-vs-C++ disagreement: 638 of 638 non-deferred difftest cases bit-identical (65 fixtures:
`my_test_bsplinesurface_nlib` offset / sweep / swung, `my_test_surface_interface` operations on
the surf1 net, Coons patches from the surf1 boundaries, the three `pt_TestFiles` `.sms` surfaces
and the 3D `.smc` curves through every op, edge cases; plus 600 generated). 27 cases are
deferred: every `CreateExtendedSurface` (`SmSurface::IsSingularity`, `surface.surface_p5`, then
`IsClosed`, `surface.surface_p7`). The file statics `sm_CreateNonRationalCurve`, `sm_MakeKnots`,
`sm_CheckTrimBoundaries` and `sm_check_zero_surface_derivs` are diffed through verbatim copies
in the shim (the file defines class members, so it cannot be compiled inside a namespace). No
`UNPORTED(nurbs.b_spline_surface_n_lib_p1)` stubs existed. Upstream behaviour worth knowing:

- `sm_CreateNonRationalCurve` of a rational curve with a base surface crashes in the C++ shim
  (segfault) even for a line lying on a bilinear plane (`DropCurveCurve.smc` with unit weights on
  `DropCurveSurface.sms`); presumably `SmSurface::DropCurve` on a surface without face/brep.
  Root cause not verified; the drop path (Rust: `UNPORTED(intersect.surface_drop_lib_p3)`) is not
  diffed.
- `CreateCoonsPatch`, `CreateNSidedPatch` and `JoinSurface` hand the NURBs of their inputs to NLib,
  which changes them in place (compatibility, rescaled cross-boundary derivatives); arrays NLib
  reallocates come from the method's local stack, so the caller's objects dangle after the call.
  The port stores the changed NURBs back; the shim never reads the inputs afterwards.
- `Reverse` reverses in v only for `SM_SP_V` (every other value reverses in u), while
  `CreateRuledSurface` / `ExpandBoundarySpan` treat only `SM_SP_U` as u.
- `CreateExtendedSurface` halves the distance after each failed extension until it is below
  `dDist / 100`, which never happens for `dDist == 0` (or NaN): upstream loops forever; the port
  stops after 1100 halvings (`NotConverged`).
- `N_SrfPowerBasisEvalDerivs` (power-basis `EvaluateSimple`) stores `min(hv, q)` rows of v-power
  derivatives after the `p + 1` u powers in one `NL_MAXDEG + 1` array and overruns it when
  `p + 1 + min(hv, q)(q + 1) + q > 17`; the FFI rejects such input.
