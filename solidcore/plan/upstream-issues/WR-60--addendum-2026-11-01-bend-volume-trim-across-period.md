Updates: WR-60

#### WR-60 (update, 2026-11-01). Opt-in periodic parameter-box trim across the seam

- **Location:** `source/SMLib/src/SmBendVolume.cpp:2171`-`2186` (`SmBendVolume::TrimParamBoundingBoxSimple`).
- **Confirmed:** yes, reproduced by difftest fixture `trim_unit_across_period` (`fixtures/surface.bend_volume/cases.json`) and unit tests in `crates/solidcore/src/surface/bend_volume.rs`.
- **Defects in pinned `v1.0.1`:** For a bend with primary `v` period $[-\pi |d|, \pi |d|]$:
  1. **Seam-crossing inversion and stale `rTrimBox` (`:2180`-`2184`):** `dVMin` and `dVMax` are clamped independently with `SM_CLAMP_TO_PERIOD`. When `[v_min, v_max]` crosses the seam `v = \pi d` (e.g. `d = 1`, `v` in `[2.0, 4.5]`), `dVMin` stays `2.0` while `dVMax` wraps to `4.5 - 2\pi = -1.7832 < dVMin`; `rTrimBox.SetMinMax(...)` rejects the inverted `v` interval, its failing status is ignored, and `TrimParamBoundingBoxSimple` returns `SM_SUCCESS` with `rTrimBox` still holding whatever stale bounds the caller passed in.
  2. **Shifted and full-period intervals (`:2175`-`2181`):** periodic intervals shifted by a nonzero multiple of $2\pi |d|$ (e.g. `[2\pi - 0.5, 2\pi + 0.5]`) are rejected with `SM_ERR` by `dVMax < dPeriodMin - SM_EFF_ZERO || dVMin > dPeriodMax + SM_EFF_ZERO`, while intervals spanning at least a full period $2\pi |d|$ (e.g. `[-8, 8]`) shrink to the wrapped endpoints instead of `[-\pi |d|, \pi |d|]`.
- **Opt-in (`Kernel::fixes.bend_volume_trim_param_box_across_period`):** default `false` preserves pinned `v1.0.1` parity in `BendVolume::trim_param_bounding_box_simple`, `BendVolume::trim_param_bounding_box_simple_with_kernel`, and `BendVolume::trim_param_bounding_box_simple_with_fixes`.
  - When enabled, negative-volume or `NaN` boxes and boxes wholly below `u = singularity_tol` initialise `trim_box` and return `SM_ERR`; `u_min` is clamped to `singularity_tol`; unbounded or full-period (`v_max - v_min >= 2 * pi * |d|`) `v` intervals expand to `[-pi * |d|, pi * |d|]`; shorter intervals outside the primary period wrap their midpoint into `[-pi * |d|, pi * |d|]` and keep the shifted interval when it does not straddle `±pi * |d|` (within `SM_EFF_ZERO`), expanding genuinely seam-straddling intervals to `[-pi * |d|, pi * |d|]`. `trim_box` is guaranteed non-inverted whenever `Ok(())` is returned.
- **Follow-up callers:** `Volume::trim_param_bounding_box_simple` (`crates/solidcore/src/surface/volume/p1.rs`) does not yet take `&Kernel` and can forward `&k.fixes` to `BendVolume::trim_param_bounding_box_simple_with_fixes` in an independent follow-up.
- **Regression tests (`surface::bend_volume::tests`):**
  - `wr60_across_period_and_shifted_period_trim`

`addendum_options: Kernel::fixes.bend_volume_trim_param_box_across_period`
