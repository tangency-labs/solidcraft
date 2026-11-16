# A curve's length: each step held to its share of the curve, not to the length so far (2026-11-08)

- **Unit:** `curve.curve_p5` (`Curve::length_base`), `mass.integrator` (`OdeOptions::rate`), #2894.
- **Cause:** the adaptive integral of `|C'|` held each step's error to the accuracy times
  `|y| + |h y'|`, the length so far plus the step's own size. Late in a span `|y|` is nearly the
  whole span, so each of its `N` steps could err by nearly the accuracy relative to all of it,
  and the steps together by up to `N` times the accuracy. The accuracy asked was also divided by
  the span count (the C++ `dDesiredAccuracy / span count`), which is not the bound the spans
  need: their lengths add, so their errors add. The parabola `(x, x^2)`, `0 <= x <= 10`, as a
  rational quadratic with weights 1, 4, 16 came out 1.05e-12 off at a requested 1e-12.
- **Change:** `OdeOptions::rate`, the integral's size per unit of `x` over the whole range. Each
  step of `h` is held to half the accuracy relative to its share of the range, `|h| rate`, plus
  its own increment `|y_(k+1) - y_k|`. The shares add up to at most `L0 + L <= 2 L` over the
  whole curve, so the steps' errors together stay within `accuracy * L` for any number of steps
  or spans (the global-error criterion of QUADPACK, Piessens et al. 1983: sum of the errors at
  most `eps |I|`). The division by the span count goes. Only the curve length sets `rate`; the
  face and edge property integrals keep their scales (`rate: None`).
- **Moved:** 59 goldens in 5 units: `curve.curve_p5` 17, `mass.integrator` 25, `curve.curve_p3` 7,
  `boolean.api_brep` 2, `blend.fillet_executive_p3` 8. The two blessing commits list every case
  with its old and new values. Every length is within the accuracy asked against an independent
  composite Gauss-Legendre reference; the arc-length searches move within their accuracy.
- **Not upstream:** the C++ divides the accuracy by the span count and holds each step against
  the length so far, so it is further from the bound than the Rust after this change. It is not
  recorded as an upstream issue here: no case was run on the C++ build, so the defect is not
  confirmed.
- **Tests:** `a_rationally_parametrised_parabola_holds_every_requested_accuracy`
  (`tests/curve_length_holds_its_accuracy.rs`), `a_rate_holds_the_steps_errors_to_the_accuracy_over_the_whole_range`
  and `a_rate_that_is_not_a_finite_nonnegative_number_is_refused` (`mass/integrator.rs`).
