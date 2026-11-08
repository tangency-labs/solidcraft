#### UB-103. `SmSrfInVolume::STEPInversion` indexes an empty point array when the volume inverse succeeds with no points
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSrfInVolume.cpp:927`-`928` (`sParamPoints[0]`), `SmSrfInVolume::STEPInversion`.
- **What happens:** After `InvEvaluatePoint` / `InvMapPoint` reports `bSuccess == TRUE` the code passes `sParamPoints[0]` to the base surface without checking that the array holds a point. A volume inverse that succeeds with no points leaves it empty, and the reference passes a null reference (segfault in `SmSurfacePointSolver`).
- **Trigger:** `STEPInversion` of a point the volume's inverse accepts but returns no point for (found by the generated `surface.srf_in_volume` cases once non-transform maps were allowed).
- **How found:** crash in the C++ reference run (difftest case `gen1653`).
- **Confirmed:** yes (segfault, exit 139).
- **Severity:** crash/UB (null dereference).
- **Rust port:** returns `SM_ERR` when there is no first point; the shim probes the same volume call and reports `SM_ERR` instead of calling `STEPInversion`.
