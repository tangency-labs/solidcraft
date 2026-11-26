#### HANG-46. `SmCurve::ApproximateCurve` never finishes for a break interval far from the origin
- **Severity:** Hang
- **Area:**
- **Location:** `SmCurve.cpp:1504` `SmCurve::ApproximateCurve` (the bisection loop around `sm_AddToCurve`; the only stop is a span shorter than `SM_EFF_ZERO`).
- **What happens:** Where coordinates reach about `1e10`, a Hermite span passes `sm_TestSpanAccuracy` only when it is tiny (about `1e-4` at `t = 5e10` with tolerance `1e-5`), so a break interval reaching `1e300` appends spans without end and the control polygon grows until memory runs out. When a failing span can no longer be split (its midpoint rounds onto an end and evaluates to that end's data), the loop repeats one state forever.
- **Trigger:** `ApproximateCurve` of a line with breaks `[-0.81, 5.69, 8.33, 1e300, 8.53]` and tolerance `1e-5` (`fuzz-out/n2000/curve.curve_p1/fuzz01255.json`); the port, which follows the same loop, aborted on a 6 GiB control-polygon allocation after 16 s.
- **How found:** fuzz-corpus replay against main (2026-10-22), located with gdb in the port.
- **Confirmed:** no (the `curve.curve_p1` FFI refuses values above its coordinate bound, so the reference build never runs it; the port's loop is a line-by-line port).
- **Severity:** hang (unbounded memory).
- **Rust port:** Returns `InvalidInput` past `MAX_APPROX_SPANS = 2^20` appended spans and when a failing span cannot be split (`curve/curve/p1.rs`).
