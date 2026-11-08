#### WR-49. Representational volume continuities leave the last knot `SM_CT_UNDEFINED`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:505`-`:518` (`SmBSplineVolume::CalculateContinuitiesSimple`, `bCalcGeometric == FALSE`).
- **What happens:** `rConts` is sized to the knot count but the loop fills only the first `count - 1` entries, so the last knot's continuity is the zero fill `SM_CT_UNDEFINED`, while the first (an end knot) gets a degree-minus-multiplicity value (`SM_CT_C0` when clamped) instead of the documented `SM_CT_DISCONTINUOUS`. The minimum therefore includes the start knot but not the end knot, and `HasDiscontinuitiesSimple(.., FALSE)` is `TRUE` for every clamped volume.
- **Trigger:** `CalculateContinuitiesSimple(SM_VP_U, .., FALSE)` on a trilinear unit cube volume: conts `[C0, UNDEFINED]`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixtures `conts_*_rep` of `surface.b_spline_volume_n_lib`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.
