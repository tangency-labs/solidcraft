Updates: WR-27

#### WR-27 (addendum 2026-11-01) — Opt-in computed second derivative in `SmConic::Evaluate` zero-tangent fallback (#2202)

- **Severity:** Wrong result
- **Area:** Curves
- **Status in the port:** `Kernel::fixes.conic_evaluate_uses_computed_second_deriv` (`#2202`, default `false`). Default `Conic::evaluate` and `Conic::evaluate_with_fixes(..., &UpstreamFixes::default())` preserve pinned v1.0.1 C++ parity on a fresh buffer (`out[2] = Vec3::UNDEF`) so `curve.conic` and `curve.curve_p2` difftests remain unchanged.
- **Location:** `source/SMLib/src/SmConic.cpp:125-281` (`SmConic::Evaluate`, specifically lines `:235-274`), at v1.0.1 (`3fd5b95`); in Rust, `crates/solidcore/src/curve/conic.rs` (`Conic::evaluate_with_fixes`, `Conic::evaluate`) and `crates/solidcore/src/curve/curve/p2.rs` (`Curve::evaluate`, `Curve::evaluate_into`, `<Conic as CurveEvaluate>::evaluate`).
- **Root cause & call-site audit:**
  1. In `SmConic::Evaluate` (`SmConic.cpp:125-281`), upstream evaluates `aPointAndDerivatives[0]` (`EvaluatePoint`), computes `sTan` and (when `lNumDerivatives > 1`) `sSec` (rotated by `-m_dThetaRad` at `:212-226`), writes `aPointAndDerivatives[1] = sTan` at `:228`, and returns early at `:230` when `lNumDerivatives == 1` (making the `if (lNumDerivatives < 2)` recursion at `:245-253` unreachable dead code).
  2. When `bNonZeroTangents` is true and `aPointAndDerivatives[1].LengthSquared() < dNewLen * dNewLen` (`dScaledZero = SM_EFF_ZERO * (1.0 + aPointAndDerivatives[0].GetMaxDimension())`, `dNewLen = 1.1 * dScaledZero`), lines `:256-259` read `aPointAndDerivatives[2].Length()` and `(dNewLen / dLen) * aPointAndDerivatives[2]` **before** line `:274` writes `aPointAndDerivatives[2] = sSec;`. On a fresh `SmVector3d[3]` array (`Vec3::UNDEF`, length `~2.14e-61 <= dNewLen`), `dLen > dNewLen` never holds, so a zero or near-zero first derivative is left unreplaced; on a reused caller buffer in C++, `aPointAndDerivatives[1]` is replaced along stale caller data.
  3. For `lNumDerivatives >= 3`, lines `:278-279` return `SM_ERR` (`Err(SolidError::sm_err(OP_EVALUATE))` in Rust, discarding `out`), so `num_derivs == 2` is the sole derivative order whose `Ok` result changes under opt-in.
  4. Across `source/SMLib` at v1.0.1 (`3fd5b95`), `SmAdvSurfaceIntersector.cpp` constructs `SmConic` Dupin indicatrix curves and calls `Transform2D` and `IntersectConic` (`ComputeImplicitEquation`, `IntersectImplicitConic`, `EvaluatePoint`, `InvertPointOnConic`), while `SmCurve` virtual `Evaluate` dispatches to `SmConic::Evaluate`. In `crates/solidcore`, `Curve::evaluate`, `Curve::evaluate_into`, and `<Conic as CurveEvaluate>::evaluate` (`curve/curve/p2.rs`) forward `&k.fixes` to `Conic::evaluate_with_fixes`.
- **Opt-in behaviour (`true`):** Reads the computed second derivative `sec` (`sSec`) instead of the unwritten `out[2]` slot in the `non_zero_tangents` fallback (`num_derivs >= 2`), scaling `sec` to `new_len = 1.1 * scaled_zero` (negated when `param > natural_interval().mid()`) whenever `|sec| > new_len`, while leaving totally collapsed conics (`|sec| <= new_len`), non-degenerate conics (`|tan| >= new_len`), `non_zero_tangents == false`, and derivative orders `0`, `1`, and `>= 3` unchanged.
- **Regression tests:** `crates/solidcore/src/curve/conic.rs::tests`:
  - `wr27_flag_lookup_and_toggle`
  - `wr27_zero_and_tiny_tangent_ellipse_uses_computed_second_deriv`
  - `wr27_hyperbola_transformed_conic_and_curve_dispatch`
  - `wr27_non_degenerate_and_other_deriv_orders_identical`
- **Tracking:** tangency-labs/solidcore#2202.
