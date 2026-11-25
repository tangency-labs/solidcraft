#### UB-64. `sm_CreateNonRationalCurve` crashes for a rational curve on a base surface
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmBSplineSurfaceNLib.cpp:2906`, `sm_CreateNonRationalCurve` (file static).
- **What happens:** Segfaults, presumably in `SmSurface::DropCurve` on a surface with no face or B-rep. The root cause was not verified.
- **Trigger:** `DropCurveCurve.smc` with unit weights (rational) on `DropCurveSurface.sms`.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The drop path is not ported yet (`Unported`).
