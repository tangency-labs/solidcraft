# surface.b_spline_volume_n_lib

- `SmBSplineVolume::CalculateContinuitiesSimple` with `bCalcGeometric == TRUE` (the
  default, and what `HasDiscontinuitiesSimple` passes by default) samples iso-parametric
  curves through `EvaluateIsoParametricCurveSimple`, which is `SM_ERR` without
  `USE_VOLUMES`; the call fails at the first sample after `rParams` holds the knots,
  `rConts` is empty and the minimum is `SM_CT_CINFINITY`. `HasDiscontinuitiesSimple`
  ignores the status, so a B-spline volume reports no discontinuities unless asked for
  representational ones. The unreachable rest of the loop samples `sKnots2[ii]` (not
  `[jj]`), reading past `sKnots2` when it has fewer samples than `sKnots1` (stale stack
  memory); the port returns `InvalidInput` there.
- Representational continuities (`bCalcGeometric == FALSE`) size `rConts` to the knot
  count but fill only the first `count - 1` entries; the last stays zero
  (`SM_CT_UNDEFINED`), and the first (an end knot) is `SM_CT_C0` for clamped knots, so
  `HasDiscontinuitiesSimple(.., FALSE)` is `TRUE` for every clamped volume.
- `SmVolume::HasDiscontinuitiesSimple`'s `bCalcGeometric` had been dropped in the Rust
  dispatch (`Volume::has_discontinuities_simple`); it is now a parameter. The
  `IsCurveIn*Domain` / `IsLineInParamDomain` volume entry points take a `&Kernel` for the
  curve bounding boxes the B-spline override computes.
