Updates: WR-27

#### WR-27 (update, 2026-11-09). `conic_evaluate_uses_computed_second_deriv` is removed

`Conic::evaluate` (upstream `SmConic::Evaluate`) orients and scales a near-zero first derivative with the computed second derivative when `non_zero_tangents` is set and two derivatives are asked for, where upstream reads the not yet written slot and so never applies the fallback. `evaluate_with_fixes` is folded into `evaluate`. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::conic_evaluate_uses_computed_second_deriv`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `curve::conic::tests::wr27_*` (zero and tiny tangent ellipses either side of the midpoint, hyperbola, dispatch through `Curve` and `CurveEvaluate`) now assert the fixed behaviour only.
