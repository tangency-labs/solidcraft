#### WR-769. A reversed `SmOffsetCurve` reports its knots in the base curve's parameterization

- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/src/SmOffsetCurve.cpp:400-409`, `SmOffsetCurve::GetKnots`, against `SmOffsetCurve::ReverseParameterization` at `:435-458`, at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
- **What happens:** `ReverseParameterization` only sets the curve's reversal flag; evaluation then mirrors each parameter `t` to `min + max - t` before evaluating the base curve. `GetKnots` passes the caller's interval and the base curve's knots and multiplicities straight through (`m_pCurve->GetKnots(rKnots, pKnotMultiplicities, pOptIvl)`). For a reversed offset, the knots therefore sit at the mirror images of their true positions and in the wrong order, and an interval query selects the wrong knots.
- **Trigger:** A quadratic B-spline base on [0, 2] with knots 0 (×3), 0.5 (×2), 1, 2 (×3), offset by 0 and reversed. The reversed curve's breaks are at 0, 1, 1.5, 2 with multiplicities 3, 1, 2, 3; `GetKnots` reports 0, 0.5, 1, 2 with 3, 2, 1, 3. A query for [1.25, 1.75] in the reversed curve's parameters should return the break at 1.5 and returns none.
- **How found:** Auditing projected queries on reversed offset curves in PR [#2840](https://github.com/tangency-labs/solidcore/pull/2840).
- **Confirmed:** yes, from the pinned source. No C++ build was run. The Rust test `reversed_offset_maps_knots_multiplicities_and_continuity_order` in `curve/offset_curve.rs` encodes the trigger and fails on the ported pass-through.

The Rust fix maps the query interval through the reversal, mirrors and reverses the base knots and multiplicities (and the continuity order), and filters the mirrored knots against the caller's interval in the caller's coordinates. Nonfinite or unordered results refuse.
