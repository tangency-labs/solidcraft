#### WR-48. `SmBSplineVolume::HasDiscontinuitiesSimple` never reports geometric discontinuities
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:242` (`HasDiscontinuitiesSimple`, def. `:226`) -> `:606` (`CalculateContinuitiesSimple`) -> `source/SMLib/src/SmBSplineVolume.cpp:2364` (`EvaluateIsoParametricCurveSimple`, `SM_ERR` in its `#else` branch).
- **What happens:** With the default `bCalcGeometric = TRUE`, `CalculateContinuitiesSimple` fails at its first iso-curve (no `USE_VOLUMES`), leaving the knots in `rParams`, `rConts` empty and the minimum `SM_CT_CINFINITY`. `HasDiscontinuitiesSimple` ignores the status and returns `FALSE` with empty continuity lists, even for a volume with `C0` knot planes.
- **Trigger:** `HasDiscontinuitiesSimple(&d)` on any `SmBSplineVolume`, e.g. one with a cubic `U` knot vector holding a triple interior knot.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixtures `hasdisc_*_geom`, `conts_*_e3_geom` of `surface.b_spline_volume_n_lib`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.
