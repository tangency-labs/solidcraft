#### WR-94. `SmIntegrator::IntegrateIt` hides integration/evaluator failure as zero
- **Severity:** Wrong result
- **Area:** Numerical integration
- **Location:** `SmIntegrator.cpp:108-122`, `SmIntegrator::IntegrateIt`.
- **What happens:** If `RungeKuttaODEIntegrate` fails, the scalar wrapper sets its
  result to `0.0` and returns `SM_SUCCESS`. A failed numerical integral is therefore
  indistinguishable from a valid zero, including for callers computing curve length.
- **Trigger:** Integrate the constant function `1` over `[0,1]` at accuracy `0`,
  or an evaluator that returns `SM_ERR` partway through the interval.
- **How found:** port source review and differential fixtures `zero_accuracy_0`
  and `evaluator_error_0` in `fixtures/mass.integrator/integrator.json`.
- **Confirmed:** yes, the actual C++ scalar wrapper reports success with zero;
  the direct ODE wrapper reports failure for the same zero-accuracy integral.
- **Severity:** wrong result / hidden numerical failure.
- **Rust port:** retained for parity; `Unported` still propagates.
