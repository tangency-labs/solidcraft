Updates: WR-94

#### WR-94 (update, 2026-11-10). `integrator_propagates_failure` is removed

`integrate` (`SmIntegrator::IntegrateIt`) returns the integrator's failure: an evaluator error, a non-finite value, an accuracy it cannot meet, step underflow or too many steps. Upstream returns 0.0 with success instead. `Curve::length` and the surface area strips (`AreaIntegrand::evaluate`) inherit this. Propagation is now the only path, and the flag is gone. Removed: the field, `integrate_opt_in`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `mass.integrator` changes in 3 cases, from 0.0 with success to status 1001:
  - `zero_accuracy_0` and `negative_accuracy_0` integrate f = 1 over [0, 1], whose value is 1.
  - In `evaluator_error_0` the evaluator fails for t >= 0.5.

  The ODE ops of the same inputs already failed. No other golden unit changes, including `curve.curve_p5` and `surface.surface_p1`.
- **Tests:** `mass::integrator` (`wr94_failed_integration_propagates`, `wr94_curve_length_and_surface_area_propagate_failure`) and `tests/curve_length_holds_its_accuracy.rs`.
