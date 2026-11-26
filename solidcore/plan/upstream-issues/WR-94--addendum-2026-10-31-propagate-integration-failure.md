Updates: WR-94

#### WR-94 (update, 2026-10-31). Opt-in failure propagation in `SmIntegrator::IntegrateIt`

- **Status in the port:** `Kernel::fixes.integrator_propagates_failure`, off by default. The default `mass::integrator::integrate` (`integrate_opt_in(..., false, ...)`) preserves the pinned v1.0.1 C++ behavior so `mass.integrator` fixtures (`zero_accuracy_0`, `evaluator_error_0`), `curve.curve_p5`, and `surface.surface_p1` remain at parity.
- **Location:** `source/SMLib/src/SmIntegrator.cpp:108-122` (`SmIntegrator::IntegrateIt`) at v1.0.1 (`3fd5b95`), plus its geometry callers `source/SMLib/src/SmCurve.cpp:14701-14709` (`SmCurve::Length`) and `source/SMLib/src/SmSurface.cpp:5263-5285` (`SmAreaIFEO::Evaluate`); in Rust, `crates/solidcore/src/mass/integrator.rs` (`integrate`, `integrate_with_kernel`, `integrate_opt_in`), `crates/solidcore/src/curve/curve/p5.rs` (`Curve::length_base_with_zone_tol`), and `crates/solidcore/src/surface/surface/p1.rs` (`AreaIntegrand::evaluate`).
- **What changes:** When `propagate_failure` / `Kernel::fixes.integrator_propagates_failure` is `true`:
  1. `integrate_opt_in` / `integrate_with_kernel` propagates any `Err` returned by `OdeIntegrator::default().integrate(...)` (an evaluator error such as `SM_ERR`, non-finite derivative or integration control, unachievable accuracy such as `0.0`, step underflow below `minimum_step`, or exceeding `max_steps`) instead of setting the scalar result to `0.0` and returning `Ok(0.0)`.
  2. Degenerate intervals shorter than `SM_EFF_ZERO` (`(a - b).abs() < EFF_ZERO`, `SmIntegrator.cpp:86-89`) still return `Ok(0.0)` without evaluating the integrand, and `SolidError::Unported` continues to propagate unconditionally under both modes.
  3. Both library callers of `mass::integrator::integrate` — `Curve::length` (`Curve::length_base_with_zone_tol` in `curve::curve::p5`) and `AreaIntegrand::evaluate` (`surface::surface::p1`) — call `integrate_with_kernel(k, ...)`, propagating failed span or strip integrals when `k.fixes.integrator_propagates_failure` is enabled.
- **Regression evidence:** Unit tests in `crates/solidcore/src/mass/integrator.rs`:
  - `wr94_zero_accuracy_and_evaluator_error_propagate_under_opt_in` checks zero/negative/NaN accuracy, mid-interval `SM_ERR` evaluator failure, non-finite (`NaN`/`INFINITY`) derivatives, unconditional `Unported` propagation, degenerate `< EFF_ZERO` intervals, and bit-identical valid integrals across forward and reversed bounds.
  - `wr94_curve_length_and_surface_area_propagate_failure` checks `Curve::length` on a circular arc/circle and `AreaIntegrand::evaluate` (plus outer `integrate_with_kernel`) on a curved `Sphere` patch and an uninitialized `BSplineSurface`, verifying default `Ok(0.0)` masking vs. opt-in error propagation and bit-identical positive arc length / surface area at valid accuracy.
  - `wr94_flag_lookup` checks `UpstreamFixes::flag_mut("integrator_propagates_failure")`.
- **Tracking:** tangency-labs/solidcore#2157.
