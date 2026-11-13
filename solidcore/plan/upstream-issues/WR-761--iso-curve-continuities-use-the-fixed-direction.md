#### WR-761. `SmIsoCurve::CalculateContinuities` reports the fixed direction's knots, not the curve's

- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/src/SmIsoCurve.cpp:64-80`, `SmIsoCurve::CalculateContinuities`, against `SmIsoCurve::GetKnots` at `:310-322`, at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
- **What happens:** An iso curve runs along the surface's varying direction. `GetKnots` asks the surface for that direction's knots: `eSP = (m_eSurfParam == SM_SP_U) ? SM_SP_V : SM_SP_U` (`:316-317`). `CalculateContinuities` instead passes the fixed iso direction `m_eSurfParam` to `SmSurface::CalculateContinuities` (`:75`). That surface call returns one continuity per unique knot of the direction it is given (`SmSurface.h:421-423`). So the continuity list describes the other direction's knots, not the curve's own.
- **Trigger:** A constant-U iso curve on a tensor B-spline surface whose U and V unique-knot counts differ. The curve has three unique knots (the V knots), but its continuity list has two entries (one per unique U knot). Where the counts happen to match, the lengths agree but the labels still describe the wrong direction's knots.
- **How found:** Issue [#3113](https://github.com/tangency-labs/solidcore/issues/3113), while porting this dispatcher in [PR #3115](https://github.com/tangency-labs/solidcore/pull/3115). The Rust port reproduced the C++ call faithfully, direction included.
- **Confirmed:** yes, from the pinned source. The two methods pass different surface directions for the same curve, and the surface contract above ties each list to its direction's knots. No C++ build was run.

The Rust fix in `curve/iso_curve.rs` uses one `varying_direction()` selector for both `knots` and `calculate_continuities`, so the continuity list has one entry per curve knot. The regressions are in `crates/solidcore/tests/iso_curve_continuity.rs`. The 99 changed `curve.iso_curve` golden records are justified in `plan/findings/curve.iso_curve--varying-direction-continuity.md`.
