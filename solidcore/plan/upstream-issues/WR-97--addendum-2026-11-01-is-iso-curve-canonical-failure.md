Updates: WR-97

#### WR-97 (addendum 2026-11-01) — Opt-in `GetCanonical` failure propagation in `SmTrimmingTools::IsIsoCurve(curve, side, low, tol)`

- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Status in the port:** `Kernel::fixes.is_iso_curve_at_propagates_canonical_failure` (`UpstreamFixes`), off by default. Default `is_iso_curve_at` and `is_iso_curve_at_with_fixes` (`!fixes.is_iso_curve_at_propagates_canonical_failure`) preserve pinned v1.0.1 C++ parity so `topology.trimming_tools` difftests remain unchanged.
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3625-3653` (`SmTrimmingTools::IsIsoCurve(SmBSplineCurve const *, SmSurfParamType, double, double)`), specifically `:3639-3641` (`SER (cpUVCurve->GetCanonical (...))`), at v1.0.1 (`3fd5b95`); in Rust, `crates/solidcore/src/topology/trimming_tools.rs` (`is_iso_curve_at`, `is_iso_curve_at_with_fixes`, and callers `is_iso_curve` and `fix_backwards_outer_loop_on_seam`).
- **What changes:**
  1. Upstream `SmTrimmingTools::IsIsoCurve(cpUVCurve, eSide, dLow, d2DTolerance)` declares return type `SmBoolean` (`int`) and writes `SER (cpUVCurve->GetCanonical (...))`. When `GetCanonical` fails (for example `SM_ERR` `1001` on an `SmBSplineCurve` without a NURB), `SER` returns that nonzero `SmStatus` directly from the `SmBoolean` function, classifying a curve whose control points cannot be read as an iso-curve (`TRUE`).
  2. `is_iso_curve_at(curve, side, low, tol_2d)` retains its four-argument signature and pinned `Ok(true)` default. The new `is_iso_curve_at_with_fixes(curve, side, low, tol_2d, fixes: &UpstreamFixes)` (wired into `is_iso_curve` and `fix_backwards_outer_loop_on_seam` via `&k.fixes`) returns the failing `BSplineCurve::canonical` `Err` directly when `fixes.is_iso_curve_at_propagates_canonical_failure` is `true`.
  3. Valid UV curves with a NURB (iso and non-iso, polynomial and rational) and unconditional port errors (`SolidError::Unported`, `SolidError::InvalidInput` for non-`SmBSplineCurve` kinds, `SolidError::PortInvariant`) behave identically under both modes. Note that in the Rust port `is_iso_curve`, `is_iso_curve_on_seam`, and `fix_backwards_outer_loop_on_seam` call `Curve::ends(k)?` before `is_iso_curve_at_with_fixes`, which already fails with `SM_ERR` on a NURB-less `BSplineCurve` in both modes.
- **Regression evidence:** Unit tests in `crates/solidcore/src/topology/trimming_tools.rs`:
  - `wr97_flag_default_off_and_lookup_by_name`
  - `wr97_nurb_less_bspline_curve_default_true_vs_opt_in_err`
  - `wr97_valid_iso_non_iso_and_non_bspline_curves_unchanged`
  - `wr97_callers_wire_fixes_and_reject_nurb_less_uv_curves`
- **Tracking:** tangency-labs/solidcore#2212.
