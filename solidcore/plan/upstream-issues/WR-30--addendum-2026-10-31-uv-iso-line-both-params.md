Updates: WR-30

#### WR-30 (update, 2026-10-31). Both UV iso lines, over the varying interval and from a zero varying origin

The record's defect is in pinned v1.0.1 `source/SMLib/src/SmIsoCurve.cpp`: `CreateUVIsoLine`
tests `SM_SP_U` at `:34` and repeats it at `:38`, so the constant-`v` branch is dead. The
constant-`u` line also takes `sDomain.GetUInterval()` although it runs along v, and starts
at `sDomain.GetMin()`.

- **What happens (default, pinned):** a constant-`u` curve gets `UV(t) = (iso, v_min + t, 0)`
  over the *u* interval: the interval is the wrong direction, and the varying coordinate is
  shifted by the domain minimum. A constant-`v` curve is `SM_ERR_INVALID_INPUT`.
- **Origin detail:** `SmLine` is `P + s*scale*V`, not `P + (s - min)*V`. Keeping the domain
  minimum in `P` therefore adds it to every returned UV; correcting only the interval would
  still return a translated line on any domain whose varying minimum is nonzero. The fix
  sets the varying coordinate of `P` to zero: `UV(t) = (iso, t, 0)` for constant `u` and
  `UV(t) = (t, iso, 0)` for constant `v`, each over the domain's other interval.
- **Fix (opt-in):** `Kernel::fixes.iso_curve_uv_line_both_params`, default `false`
  (pinned behaviour). The kernel-threaded `IsoCurve::create_uv_iso_line_with_nurb` forwards
  it; the kernel-free `IsoCurve::create_uv_iso_line` has no kernel and always keeps the
  pinned body, including the constant-`v` refusal. The builder's default branch is the
  pinned body unchanged.
- **Tests:** `uv_iso_line_default_is_pinned`, `uv_iso_line_opt_in_uses_each_varying_interval_and_origin`,
  `uv_iso_line_opt_in_equal_domain_control` and `uv_iso_line_opt_in_translated_domain_origin`
  in `crates/solidcore/src/curve/iso_curve.rs`. They compare real UV samples (endpoints and
  interior), the line's dimension and natural interval, the lift through the iso curve's
  surface accessor against the iso curve evaluated at the same parameter, and an independent
  bilinear formula, on the asymmetric `[0, 2] x [-1, 3]` patch, a zero-origin equal-interval
  control, and a patch translated to `[1, 3] x [2, 6]`.
- **Confirmed:** read at the pin (`3fd5b95`, `SmIsoCurve.cpp:34`, `:38`); the pinned
  behaviour is retained by default. The opt-in path intentionally differs from the pinned
  reference and was not compared against a C++ run. No difftest case changes; WR-30 is a
  record ID, not an issue number, so no numeric issue is closed here.
