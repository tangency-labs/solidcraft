# curve.curve_p1: ApproximateCurve's shortest tested span is relative to the range it fits (2026-11-25)

- **Unit:** `curve.curve_p1`, `test_span_accuracy` and its caller `Curve::approximate_curve_base` (`crates/solidcore/src/curve/curve/p1.rs`). The upstream defect is WR-21657 in `plan/upstream-issues/`.
- **Problem (tangency-labs/solidcraft#21657):** a lift of the same UV line, parameterised over `[0, 1/f]`, was fitted within its tolerance `1e-6` at `f = 1e-3` and `1` and reported `1.8955e-3` at `f = 1e3`.
- **Cause:** a span shorter than 1/1500 of the curve's natural interval was accepted without its test. A curve on surface has its UV curve's natural interval, `[0, 1]` for a line from two points, at any speed. Over `[0, 1e-3]` the limit is about `6.7e-4`, so after the first halving every span passed untested.

![Two columns. Left, before: the fitted interval, a dashed limit two thirds as long, one span tested and halved, then two spans shorter than the limit accepted untested, and the fit reports 1.9e-3. Right, after: the same interval, a limit too short to draw, one span tested and halved, then spans halved until each passes, and the fit is within 1e-6.](curve.curve_p1--21657-shortest-tested-span.svg)

*Look at the bottom row: with the limit taken from the natural interval, the halves are accepted without their test.*

- **Change:** the limit is 1/1500 of the range the breaks cover, so it no longer changes with the parameter's scale. A fit over its whole natural interval keeps its spans.
- **Not changed:** spans shorter than the limit are still accepted with their sampled error reported, and the fixed `EFF_ZERO` floors stay; a certified bound for the fit would replace both.
