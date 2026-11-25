Updates: WR-663

#### WR-663 (update, 2026-11-01). `SmBSplineCurve::AssertValid` and `HasInternalPole` statuses

The remaining `Kernel`-taking `SmBSplineCurve` sites from `tangency-labs/smlib` `8843967` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`, left for follow-up in #2066) are now ported under `Kernel::fixes.query_statuses_pass_on` (default `false`, upstream's behaviour):

- **Tests 5/6 (`AssertValid`, interior multiple-knot derivatives):** when either the lower (`from_left = false`) or upper (`from_left = true`) first-derivative evaluation at an interior knot of multiplicity `p` fails, test 5 (2D) or 6 (3D) fails with `value = SM_UNDEF_DOUBLE` and `owner_param = (param, 0.0)`, instead of measuring `Vec3::UNDEF` (`length = sqrt(3) * SM_UNDEF_DOUBLE`).
- **Tests 11–14 (`AssertValid`, bounding box, property analyses, and solution evaluation):**
  - `CalculateBoundingBox` status (`bSized`): when unsized, reports test 11 (2D) / 12 (non-2D) with `(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE)`, sets `size_xyz = 0.0`, and skips the first-derivative, mapped-trim, and radius-of-curvature analyses (for a constructed Rust `BSplineCurve` whose NURB passed test 0, normal-box sizing over the natural interval already succeeds).
  - `GlobalPropertyAnalysis` statuses (`bFirstDerivs`, `bRadii`): a failed `SM_CP_MINIMIZE_FIRST_DERIVATIVE` or `SM_CP_MINIMIZE_RADIUS_OF_CURVATURE` solve reports test 11/12 (`(dTol, SM_UNDEF_DOUBLE)`) or 13/14 (`(dRadiusTol, SM_UNDEF_DOUBLE)`) instead of swallowing the failure as an empty solution list, and a failed radius solve skips reading `rSolutionArray` and zero-speed end stretches (`WR-668`). Partial first-derivative solutions found before a failure are still checked via `global_property_analysis_into`, as in C++.
  - Solution derivative evaluation (`bEvaluated`): a failed `Evaluate` at an interior `MinimizeFirstDerivative` solution fails test 11/12 with `value = SM_UNDEF_DOUBLE` instead of measuring `Vec3::UNDEF.length()`.
- **`HasInternalPole`:** returns `Ok(false)` when `knots(None)` fails (propagating `Unported`/`Cancelled`), matching the fork's `GetKnots(...) != SM_SUCCESS` guard.
- **Confirmed:** `nurbs::b_spline_curve::p3::tests::issue_2009_assert_valid_unevaluable_interior_knot_derivs_fail_tests_5_6` and `issue_2009_assert_valid_failed_property_analyses_fail_tests_11_to_14` verify both flag-off parity and flag-on failure reports across 2D and 3D curves.
