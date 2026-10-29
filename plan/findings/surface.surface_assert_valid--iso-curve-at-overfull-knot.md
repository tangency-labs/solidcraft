# surface.surface_assert_valid: iso curve at a knot of multiplicity above the degree (2026-10-29)

Unit: `surface.surface_assert_valid` (found there); the divergence is in
`nurbs.b_spline_surface_p2` (`BSplineSurface::create_iso_parametric_curve`).

Inputs: a B-spline surface with an interior knot whose multiplicity exceeds the degree,
e.g. degree 1 x 1, `knots_u = [0, 0, 0.5, 1, 1]`, `knots_v = [0, 0, 1/3, 1/3, 1, 1]`,
3 x 4 control points. `SmSurface::AssertValid` test 4 (`sm_AssertTestSurface4`) runs
`CalculateContinuities(SM_SP_U)`, which builds iso curves at the v knots, including
`v = 1/3`.

Values: C++ `CreateIsoParametricCurve(SM_SP_V, 1/3)` returns `SM_SUCCESS`;
`CalculateContinuities(SM_SP_U)` returns `SM_SUCCESS` with minimum continuity
`SM_CT_C0`, so test 4 reports "Bad Surf Discontinuity". The port's
`create_iso_parametric_curve` returns `Upstream { code: 1001 }`, `calculate_continuities`
stops there, and the check (which ignores the status, keeping the minimum reached so
far, `SM_CT_C1_C2_C3`) does not report test 4.

Cause: `N_SrfExtractIsoCrv` returns its error flag when the multiplicity exceeds the
degree (`mlv > q`). `SmBSplineSurface::CreateIsoParametricCurve` only prints
`ERR_MSG` and goes on with the curve `sm_AllocateNurbCurve` allocated with
`smos_Calloc`: all control points and knots zero (zero weights). The port maps the
NLib error to a status instead (`map_err(SolidError::nl_ser(..))`).

Effect on the gate: the difftest FFI rejects B-spline surfaces with an interior knot of
multiplicity above the degree (`iso_extraction` in
`crates/solidcore-ffi/src/units/surface_surface_assert_valid.rs`), so
`SmBSplineSurface` test 1 ("Bad Knot Multiplicity") is covered by the solidcore unit
tests only. Porting the zero-curve continuation in `create_iso_parametric_curve`
(and checking what its zero-weight evaluation gives) would let those cases be compared.

PR: the `port: SmSurface::AssertValid ...` PR (WP3a of #1419).
