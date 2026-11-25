#### UB-174. `sm_ExtendSurface` can retry `N_SrfEvalPtDerivsUnbounded` with an uninitialised flag
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:517`, `sm_ExtendSurface` (`NL_FLAG gflg` uninitialised); `NL_SrfAdv.cpp` `N_SrfEvalPtDerivsUnbounded`.
- **What happens:** The first call copies the surface into `esur` before `N_SrfIsClosedSmooth`. When that test fails before it stores the closed directions (a knot or evaluation error), the retry finds `esur` set and reads the uninitialised `gflg` to decide between wrapping and extending.
- **Trigger:** An offset extension over a surface whose closedness test fails with a knot or evaluation error.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** The extension state stays empty after such errors (`nlib.srf_adv_p1`), so the retry starts over.
