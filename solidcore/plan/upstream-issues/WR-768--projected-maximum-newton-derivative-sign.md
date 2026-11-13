#### WR-768. `SmCurvePropertyEFO::Evaluate` negates a projected maximum's equation but not its derivative

- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/src/SmCurve.cpp:15577-15578`, the `SM_CP_PROJECTED_POINT_MAXIMIZE` branch of the projected-point evaluator, at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
- **What happens:** For a projected point query the evaluator returns `F(t) = unit(TanProj) . VProj` and its derivative `F'(t)` (`:15565-15569`). For a maximum it then negates `F` (`rdFOfT = -rdFOfT; // [B543]`) but leaves `rdFPrimeOfT` unchanged. The local 1D solver's Newton step `t - F/F'` therefore divides the negated function by the derivative of the un-negated one, so it steps away from the root it is refining.
- **Trigger:** The line `C(t) = (t, 0, 0)` for t in [0, 1], point P = (0, 0, 0), projection normal N = (0, 0, s). The maximum's equation is `F(t) = -s² t`, whose derivative is `-s²`. The evaluator returns `F'(t) = +s²` at every t, for s = 0.5, 1 and 2.
- **How found:** Auditing projected curve maxima in PR [#2840](https://github.com/tangency-labs/solidcore/pull/2840).
- **Confirmed:** yes, from the pinned source. No C++ build was run. The Rust test `projected_maximum_equation_has_its_actual_derivative` in `curve/curve/p4.rs` encodes the trigger: it fails on the ported evaluator and passes when the derivative is negated with the function.

The Rust fix negates `F'` together with `F` for `ProjectedPointMaximize`. The corrected Newton step then reaches the nearest stationary point of the projected distance, a minimum as readily as a maximum, or runs off where the equation turns back between the guess and its root. So `local_property_analysis` accepts the root only where the equation's derivative is not negative, which is where the distance has a maximum, and otherwise samples the interval for a bracketed interior maximum and polishes that one. A corner of the distance, where the equation jumps from negative to positive without a root (a polyline vertex), is bisected to its parameter and reported as the maximum it is.
