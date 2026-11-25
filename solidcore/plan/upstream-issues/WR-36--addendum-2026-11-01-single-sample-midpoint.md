Updates: WR-36

## Opt-in single-sample midpoint fix (`Kernel::fixes.curve_max_distance_single_sample_midpoint`)

PR #2203 adds `Kernel::fixes.curve_max_distance_single_sample_midpoint` (default `false` for C++ parity):

- **`Curve::curve_max_distance_between` (`crates/solidcore/src/curve/curve/p6.rs`, upstream `SmCurve::CurveMaxDistanceBetween` at `SmCurve.cpp:2598`):**
  When `num_samples == 1` (and the single-span degree-1 line/line two-sample override does not trigger), upstream evaluates `dPer = (double)ii / (double)(lNumSamples - 1) = 0.0 / 0.0 = NaN`, leaving `dTGuess1 = NaN` across both endpoint perturbations (`0.001` and `0.999`). For non-B-spline or borrowed receivers (`HermiteCurve`, `CrvOnSurf`, etc.), every local solve fails and the routine returns `distance == 0.0` even for widely separated curves; for unborrowed B-spline receivers, the `global_point_solve` fallback drops `other(0.999)` instead of sampling the midpoint. With `curve_max_distance_single_sample_midpoint` enabled, `num_samples == 1` sets `per = 0.5` so both `interval.evaluate(per)` and `other_interval.evaluate(per)` / `other_interval.evaluate(1.0 - per)` evaluate at the interval midpoints.
- **`CrvOnSurf::surface_curve_max_distance_between` (`crates/solidcore/src/curve/crv_on_surf.rs`, upstream `SmCrvOnSurf::SurfaceCurveMaxDistanceBetween` at `SmCrvOnSurf.cpp:1780`):**
  When `num_samples == 1`, upstream likewise computes `dPer = 0.0 / 0.0 = NaN`, causing the first `sm_EvalDistAndDev` evaluation to fail and falling back to the `ii == 0` near-start sample `interval.evaluate(0.001)` (with `dTGuess2 = NaN`). With `curve_max_distance_single_sample_midpoint` enabled, `num_samples == 1` sets `per = 0.5` and evaluates the gap and lateral deviation at the interval midpoints.
- **`bDoPrecise = FALSE` in `SmCrvOnSurf::SurfaceCurveMaxDistanceBetween` (`SmCrvOnSurf.cpp:1909`):**
  Upstream explicitly disables `bDoPrecise = FALSE` with the comment `"Currently, the following two CurveSolve() methods are not necessarily working correctly, so just use what the iteration gave us"` (the unexecuted refinement block passes mismatched parameter slots `dOtherTMaxDev` / `&dThisTMaxDev` and a frozen normal `sNorm`, and was intentionally left unported in `crv_on_surf.rs`). `num_samples == 0` continues to run the dense `22 + this_knots + other_knots` sample pass without invoking that broken refinement block.

Regression tests:
- `curve::curve::p6::tests::wr36_flag_lookup_and_toggle`
- `curve::curve::p6::tests::wr36_curve_max_distance_single_sample_hermite_default_vs_midpoint`
- `curve::curve::p6::tests::wr36_curve_max_distance_single_sample_bspline_default_vs_midpoint`
- `curve::curve::p6::tests::wr36_curve_max_distance_other_sample_counts_unchanged`
- `curve::crv_on_surf::tests::wr36_surface_curve_max_distance_single_sample_default_vs_midpoint`
- `curve::crv_on_surf::tests::wr36_surface_curve_max_distance_other_sample_counts_unchanged`
