#### HANG-47. `SmFindPCExtremaEFO::Evaluate` loops forever on an infinite tangent
- **Severity:** Hang
- **Area:**
- **Location:** `SmCurve.cpp:18304` (`SmFindPCExtremaEFO::Evaluate`, def. `:18258`): `while (sPV[1].LengthSquared() > 1.0e4)` divides the derivatives by ten.
- **What happens:** an infinite tangent stays infinite, so the loop never ends.
- **Trigger:** `GlobalCurveSolve` of a rational B-spline with `-1.8e308` weights (`fuzz-out/n2000/curve.curve_p9/fuzz00144.json`, `fuzz01222.json`).
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504), located with gdb in the port.
- **Confirmed:** no (the `curve.curve_p9` FFI refuses mixed rationality, so the reference build never runs it; the port's loop is a line-by-line port).
- **Severity:** hang.
- **Rust port:** `InvalidInput` after `MAX_PC_SCALE_STEPS = 400` divisions (a finite tangent needs at most 310; `curve/curve/p3.rs`).
