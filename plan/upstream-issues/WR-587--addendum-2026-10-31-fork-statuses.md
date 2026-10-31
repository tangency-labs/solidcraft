Updates: WR-587

#### WR-587 (update, 2026-10-31). The area normal returns the fork's statuses

- **Fork change:** the SolidCraft fork's form of this fix is tangency-labs/smlib@be45e8d (unchanged to eaa998a), "curves: a loop's normal is its area vector, so a draft leans the way it is asked". Its sampled fan sum is the one `Kernel::fixes.curve_loop_normal_from_area` ported (#1779). Its two status paths were not:
  - `SER(pCrv->EvaluatePoint(..))`: a failed sample evaluation is the call's status. The port kept the previous point, upstream's default path's ignored status.
  - `SER(crCrvsNormal.Unitize())`: a zero area (no curves, or collinear ones) is `SM_ERR_INVALID_INPUT`. The port returned `(0, 0, 1)`.
- **Rust port:** under the same flag, `curve::curve::p3::compute_crvs_area_normal` now evaluates with the status propagated and unitizes with `SM_ERR_INVALID_INPUT` for a zero area, bit for bit as be45e8d. Off, nothing changes: upstream's weighted turns, statuses ignored, `(0, 0, 1)`.
- **Tests** (`curve::curve::p3::tests`):
  - `area_normal_reports_a_zero_area_and_keeps_the_single_line_error`: no curves and two collinear lines are `InvalidInput` on, `(0, 0, 1)` off; a single line is `SM_ERR` on both.
  - `area_normal_returns_a_failed_point_evaluation`: a composite curve's `SmCurve::EvaluatePoint` fails (`SM_ERR`); on, the call fails with it; off, it succeeds.
- **Difftest:** none; the default path is unchanged.
