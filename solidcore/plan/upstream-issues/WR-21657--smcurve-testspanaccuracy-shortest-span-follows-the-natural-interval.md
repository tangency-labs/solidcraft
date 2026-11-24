#### WR-21657. `sm_TestSpanAccuracy` force-passes spans shorter than 1/1500 of the curve's natural interval, not of the interval being fitted, so `ApproximateCurve` depends on the parameter's scale

- **Severity:** Wrong result
- **Area:** Curves (`SmCurve::ApproximateCurve`, and `LiftCurve` through the `SmCrvOnSurf` override)
- **Location:** `source/SMLib/src/SmCurve.cpp`, `sm_TestSpanAccuracy` (def. `:235`): `sIvl = crCurve.GetNaturalInterval()` (`:258`) and `if(dThisSpan < (sIvl.GetLength())/ dMinStep )` with `dMinStep = 1500` (`:259`, `:269`); called from `SmCurve::ApproximateCurve` (`:1732`).
- **What happens:** a span shorter than 1/1500 of the curve's natural interval is accepted without its distance test (its error is still sampled and reported). `ApproximateCurve` fits the interval between its first and last break, which need not be the natural interval.
  - When the fitted interval is much shorter than the natural interval, the first halvings already fall under the limit, and the fit keeps Hermite spans far outside the tolerance.
  - For a curve on surface the natural interval is its UV curve's. A UV line made from two points has the natural interval `[0, 1]` whatever the speed of the parameterisation the caller lifts it over.
- **Trigger:** `LiftCurve` on the biquadratic bump `S(u, v) = (2u, 2v, 4uv(1 - u)(1 - v))` over `[0, 1]²` of the UV line from `(0.1, 0.2)` along `(0.7, 0.5)·f` over `[0, 1/f]`, tolerance `1e-6`: the same segment at every `f`. At `f = 1e3` the fitted interval is `1e-3` long, the limit `1/1500 ≈ 6.7e-4`, and every span after the first halving is accepted untested.
- **How found:** #21657, found while fixing #20239.
- **Confirmed:** by test, `surface::surface::p11::tests::lift_curve_is_within_the_tolerance_at_any_parameter_speed`, which fails with the ported limit at `f = 1e3` and passes at `1e-3` and `1`. The Rust port (`crates/solidcore/src/curve/curve/p1.rs`, `test_span_accuracy`) read `curve.natural_interval()` as the C++ does.
- **Fix:** `test_span_accuracy` takes the length of the fitted interval (`fit_length`), and `Curve::approximate_curve_base` passes the distance from its first to its last break. The limit is then relative to the interval being fitted, so a fit of the same curve does not change with the parameter's scale.
- **Rust port:** deviation taken.
