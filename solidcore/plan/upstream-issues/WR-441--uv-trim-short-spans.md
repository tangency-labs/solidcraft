#### WR-441. Trimming a UV curve removes short spans using a 3D tolerance (#1114)
- **Severity:** Wrong result
- **Area:** Curves / trimming
- **Location:** `source/SMLib/src/SmBSplineCurveNLib.cpp:4624`, `SmBSplineCurve::Trim` (definition at 4469), calling `N_CrvRemoveDegenSegs` at 4635.
- **What happens:** the end-span cleanup measures arc length in the curve's own coordinate units against `SmTol::GetZoneTol3d`. For a dimension-2 UV curve these are surface parameter units, so removing a short span can move the lifted curve farther than the 3D tolerance and invalidate a Boolean's edge/trim agreement.
- **Trigger:** a rational semicircle of radius 0.14 with a double interior knot at 0.5, trimmed to `[0.4999965,0.9]` or `[0.1,0.5000035]`. On `S(u,v) = (38u,38v,0)` the default head trim has a sampled gap `4.2993900351216975e-5` against a zone tolerance of `1e-5`. The full stadium-ring trigger is documented in issue #1114.
- **How found:** issue #1114; adversarial native fixtures `issue1114_dim2_head_notify`, `issue1114_dim2_tail_notify` and their silent/interval-only and dimension-3 controls.
- **Confirmed:** yes; all twelve `issue1114_*` fixtures compare the default Rust output with the unchanged pinned C++ reference in PR #1122 (2,338/2,338 unit cases pass, maximum relative error zero). The matching control points and knots reproduce the above lifted discrepancy.
- **Rust port:** opt-in `Kernel::fixes.bspline_trim_preserves_uv_spans` applies degenerate-span removal only to dimension-3 model curves. The default preserves pinned SMLib behavior. Enabled Rust tests preserve the restricted curve within `1e-12` after lifting.
