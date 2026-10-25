#### UB-27. `N_FitCrvFirstDerivAndKnots` writes `M[2i+1][-1]`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_CrvFit.cpp:7923`, `N_FitCrvFirstDerivAndKnots`.
- **What happens:** When the span of `u[i]` gives `l == 0`, the routine writes column `-1` of row `2i+1`. The banded matrix is one row-major block, so this overwrites the last entry of row `2i` with a wrong value.
- **Trigger:** Knots for which `u[i]` falls in a span giving `l == 0` (not produced by its driver `N_FitCalcKnotsDerivs`).
- **How found:** adversarial / edge-case fixture `first_derivs_row_underflow_p2`.
- **Confirmed:** yes. The C++ matches the port bit for bit through the bad write.
- **Severity:** crash/UB (out-of-row write; silently wrong matrix).
- **Rust port:** Writes exactly there so that results match.
