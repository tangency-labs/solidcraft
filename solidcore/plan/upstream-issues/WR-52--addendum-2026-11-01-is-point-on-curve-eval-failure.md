Updates: WR-52

#### WR-52 (update, 2026-11-01). Opt-in propagation of curve evaluation, drop, and point-solve failures

At the pinned commit `3fd5b95` (usd-brep v1.0.1), `source/SMLib/src/SmSurfaceIntersector.cpp`
uses `SER(status)` inside two functions whose return type is `SmBoolean`:

1. `SmSurfaceIntersector::IsPointOnCurve` (`SmSurfaceIntersector.cpp:7350`):
   - `:7376`: `SER(pCurve->EvaluatePoint(sIvl.GetMin(), sCrvPnt))` in the degenerate-curve
     branch (`pCurve->IsDegenerate()`).
   - `:7391`: `SER(pCurve->DropPoint(..., SM_SO_INTERSECT))` in the non-degenerate-curve
     branch.
2. `sm_IsPointOnCurve` (`SmSurfaceIntersector.cpp:4097`, `:4195`):
   - `:4195`: `SER(sHerm3d.LocalPointSolve(sHermiteDomain, SM_SO_MINIMIZE, ...))`.

(`SmSurfaceTracer::IsPointOnCurve` in `source/SMLib/src/SmSurfaceTracer.cpp:192` checks
`!= SM_SUCCESS` and `continue`s rather than using `SER`, so it is unaffected.)

Because every error `SmStatus` (`SM_ERR = 1001`, `SM_ERR_NULL_POINTER = 1006`, ...) is
non-zero, `SER(...)` converts a failing evaluation, drop, or point solve into a non-zero
`SmBoolean` that callers test as `TRUE` ("point is on the curve").

The Rust port (`crates/solidcore/src/intersect/surface_intersector/p1.rs`) keeps pinned
C++ behaviour by default (`or_true` maps non-`Unported` errors to `Ok(true)`, while
`SolidError::Unported` always propagates) and adds
`Kernel::fixes.is_point_on_curve_propagates_eval_failure` (default `false`), which
propagates the underlying `Err(SolidError)` from `or_true` across all three `WR-52`
sites (`SurfaceIntersector::is_point_on_curve` / `is_point_on_curve_cached` at `:7376`
and `:7391`, and `is_point_on_tsect_curve` at `:4195`).

- **Rust measurement (opt-in path).** Verified by the `intersect.surface_intersector_p1`
  unit tests in `crates/solidcore/src/intersect/surface_intersector/p1.rs`:
  - `wr52_flag_lookup_and_default_off`: checks that
    `UpstreamFixes::default().is_point_on_curve_propagates_eval_failure` is `false`, that
    `UpstreamFixes::flag_mut("is_point_on_curve_propagates_eval_failure")` toggles it, and
    that `or_true` maps `SM_ERR` / `SM_ERR_NULL_POINTER` to `Ok(true)` when `false` and
    propagates `Err(...)` when `true` while propagating `SolidError::Unported` in both
    modes.
  - `wr52_is_point_on_curve_degenerate_eval_failure` (`:7376`): tests both a degenerate
    `Ellipse::empty(3)` without NURB (`IsDegenerate() == true`, `EvaluatePoint` -> `SM_ERR`)
    and `OffsetCurve::empty()` (`IsDegenerate() == true`, `EvaluatePoint` ->
    `SM_ERR_NULL_POINTER`) against a far point `(100, 100, 100)`. With the fix off, both
    return `Ok(true)`; with the fix on, they propagate `SM_ERR` (`1001`) and
    `SM_ERR_NULL_POINTER` (`1006`) respectively; `skip = Some(0)` returns `Ok(false)` in
    both modes.
  - `wr52_is_point_on_curve_nondegenerate_drop_failure` (`:7391`): tests a non-degenerate
    `Ellipse` (`r = 1`, `[0, 90]` deg) without NURB or polar conversion table
    (`IsDegenerate() == false`, `DropPoint` -> `SM_ERR_NULL_POINTER`) via both
    `is_point_on_curve` and `is_point_on_curve_cached`, including multi-curve slices where
    an earlier hit short-circuits to `Ok(true)` and an earlier miss reaches the broken
    curve.
  - `wr52_valid_curves_and_tsect_span_parity`: verifies that valid degenerate curves
    (`BSplineCurve::create_degenerate_curve` and zero-span `HermiteCurve`), valid
    non-degenerate `Line` segments, and `is_point_on_tsect_curve` spans return identical
    `Ok(true)` / `Ok(false)` results with the flag off and on.
- **Limits.** In `sm_IsPointOnCurve` (`:4195`), `SmCurve::LocalPointSolve` with
  `SM_SO_MINIMIZE` (`SmCurve.cpp:19256-19267`, `curve::curve::p3`) catches any
  `LocalSolve1d::solve_it` failure and returns `Ok((false, solution))`, and
  `SmHermiteCurve::EvaluatePoint` is infallible, so `:4195` cannot return `Err` end-to-end
  under current `HermiteCurve` and `LocalPointSolve` semantics; its `or_true(k, e)` gate is
  aligned with `:7376` and `:7391` and exercised directly via `or_true`. Default difftest
  parity and unit status are unchanged.
