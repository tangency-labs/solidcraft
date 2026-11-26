#### UB-87. `SmCone::CreateIsoCircleFromPoint` and `GetHeight` read uninitialised parameters
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmCone.cpp:1679`-`1682` `SmCone::CreateIsoCircleFromPoint` (def. `:1620`); `SmCone.cpp:2265`-`2266` `SmCone::GetHeight` (def. `:2251`).
- **What happens:** `CreateIsoCircleFromPoint` ignores the statuses of `STEPInversion` and `ConvertUVFromSTEPToNURBS`; when the generator drop in `SmSurfOfRevolution::STEPInversion` finds no solution the height of `sStepUV` is never written and is used anyway. `GetHeight` also ignores `EvaluatePoint`'s status when the conversion fails (no polar curve), but that part is defined: the `SmPoint3d` outputs keep their `SM_UNDEF_DOUBLE` member initialisers, so two failures give height `0` (PR #226, issue #277).
- **Trigger:** `CreateIsoCircleFromPoint` with a reference point just outside the height interval of a steep bounded cone and a tolerance below the in-plane gap the clamp leaves; `CreateIsoCircleFromPoint` on `SmCone::CreateCanonical(O, 1, 5e-11 deg, NULL)` (no `bSwapUV`) at `(-1, 1e-13, 0.7)`, where the polar conversion fails and `sNurbUV.y` (a defaulted `SmVector2d`) is used unset; `GetHeight` of the cone of UB-81.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim replays the inversion and refuses those inputs.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** `CreateIsoCircleFromPoint` returns the error. `Cone::height` mirrors `GetHeight` (failed evaluations keep `Vec3::UNDEF`), and the `surface.cone_p2` difftest compares it, and the offset and trim ops that use it, on cones without a polar curve (`fixtures/surface.cone_p2/no_polar_curve.json`).
