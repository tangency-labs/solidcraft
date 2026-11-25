#### HANG-8. `SmBSplineCurve::DampenKnotSpacing` cascades without end
- **Severity:** Hang
- **Area:**
- **Location:** `SmBSplineCurveNLib.cpp:1082`, `SmBSplineCurve::DampenKnotSpacing`.
- **What happens:** Each pass halves every span that is more than `dMaximumDeltaFactor` times longer than a neighbour. For factors below about 2 the halving cascades without end, and a factor of 1 or less never finishes.
- **Trigger:** Unique knots `0 .3 .6 1`, factor `1.2`.
- **How found:** read during porting.
- **Confirmed:** no. The harness simulates the splitting and rejects such inputs.
- **Severity:** hang.
- **Rust port:** Stops after `DAMPEN_SPLIT_LIMIT` splits with `NotConverged`.
