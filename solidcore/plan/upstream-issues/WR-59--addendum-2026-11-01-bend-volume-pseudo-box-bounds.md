Updates: WR-59

#### WR-59 (update, 2026-11-01). Opt-in exact mid-angle pseudo box and unbounded-`u` downward `dYMax`

- **Location:** `source/SMLib/src/SmBendVolume.cpp:1541`-`1573` (`SmBendVolume::EvaluateBoundingBoxSimple`, `pOptProjPseudoBox` branch) and `:1475`-`1481` (axis-aligned `pOptProjBox`, `dUMax == SM_INFINITE_PARAMETER`, `!(dThetaMin >= 0.0 || dThetaMax > 0.0)`).
- **Confirmed:** yes, reproduced by difftest fixtures `b_box_unit_both_poles` and `b_box_unit_inf_u_down` (`fixtures/surface.bend_volume/cases.json`) and unit tests in `crates/solidcore/src/surface/bend_volume.rs`.
- **Defects in pinned `v1.0.1`:** For the simple bend map $P(u, v, w) = (u \cos(v / d), u \sin(v / d), w)$:
  1. **Mid-angle pseudo box (`:1549`-`1557`):** in the orthonormal basis $(\cos\theta_{\text{mid}}, \sin\theta_{\text{mid}}, 0)$, $(-\sin\theta_{\text{mid}}, \cos\theta_{\text{mid}}, 0)$, $(0, 0, 1)$, a mapped point has coordinates $(u \cos\phi, u \sin\phi, w)$ with $\phi = v / d - \theta_{\text{mid}} \in [-\theta_{\text{end}}, \theta_{\text{end}}]$. Upstream comments out `dCosThetaEnd = smos_Cosine(dThetaEnd)`, sets `dXMin = dThetaEnd < SM_PI ? dSinThetaEnd * dUMin : dSinThetaEnd * dUMax`, and sets `dYMax = dThetaEnd < SM_PI ? dSinThetaEnd * dUMax : dUMax`:
     - Uses `sin(theta_end)` instead of `cos(theta_end)` for `dXMin`, and switches at `SM_PI` instead of `SM_PI / 2`: for $\pi / 2 < \theta_{\text{end}} < \pi$ (e.g. `d = 1`, `u` in `[0.5, 1.5]`, `v` in `[-2, 2]`), `dXMin = sin(2) * 0.5 = 0.4546` while the corner `(1.5, 2, 0)` maps to $x = 1.5 \cos(2) = -0.6242$, and for $\theta_{\text{end}} = \pi$ (`v` in `[-\pi d, \pi d]`), `dXMin = sin(\pi) * u_{\max} \approx 0`, missing the entire negative-`x` half-annulus `[-u_{\max}, 0]`.
     - Compares `dThetaEnd` against `SM_PI` instead of `SM_PI / 2` for `dYMax`: for $\pi / 2 < \theta_{\text{end}} < \pi$, `dYMax = sin(theta_end) * u_max < u_max`, missing the boundary/interior points at $\phi = \pm\pi / 2$ where $y = \pm u_{\max}$.
     - Multiplies `SM_INFINITE_PARAMETER` by `sin(theta_end)` when `u` is unbounded, dropping the `±SM_INFINITE_PARAMETER` unbounded marker.
  2. **Unbounded-`u` axis-aligned `dYMax` (`:1480`):** when `dUMax == SM_INFINITE_PARAMETER` and both end angles are `<= 0`, upstream sets `dYMax = smos_Max(dCosThetaMin, dCosThetaMax) * dUMin` instead of `smos_Max(dSinThetaMin, dSinThetaMax) * dUMin`.
- **Opt-in (`Kernel::fixes.bend_volume_bbox_contains_wide_angle_and_inf_u`):** default `false` preserves pinned `v1.0.1` parity in `BendVolume::evaluate_bounding_box_simple`, `BendVolume::evaluate_bounding_box_simple_with_kernel`, and `BendVolume::evaluate_bounding_box_simple_with_fixes`.
  - When enabled, `proj_box` uses `max2(sin_theta_min, sin_theta_max) * u_min` on the downward unbounded-`u` branch, and `proj_pseudo_box` computes `cos_min` (`cos(half_angle)` for `half_angle < pi`, `-1.0` otherwise) and `sin_max` (`sin(half_angle)` for `half_angle < pi / 2`, `1.0` otherwise) over `[u_min, u_max]`, propagating `±SM_INFINITE_PARAMETER` along unbounded `u` rays and leaving `proj_pseudo_box` untouched when `param_box` has negative volume or `bend_neutral_dist` is zero/non-finite.
- **Follow-up callers:** `Volume::evaluate_bounding_box_simple` (`crates/solidcore/src/surface/volume/p2.rs`) currently calls `BendVolume::evaluate_bounding_box_simple` and can forward `k: &Kernel` to `BendVolume::evaluate_bounding_box_simple_with_kernel` in an independent follow-up.
- **Regression tests (`surface::bend_volume::tests`):**
  - `wr59_wide_angle_pseudo_box_contains_image`
  - `wr59_unbounded_u_downward_angles_and_flag_lookup`

`addendum_options: Kernel::fixes.bend_volume_bbox_contains_wide_angle_and_inf_u`
