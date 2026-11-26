#### UB-67. Surface helpers read empty arrays or unset counts
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurface.cpp:24009` `sm_SetUpBreaks` (reads `rBreaks[0]` of an empty array); `:17963` `SmLSIGlobalSolver::BranchMayContainAnswers` (uses an unset hit count when `SmExtent3d::IntersectLine` fails on a zero line vector); `SmSurfaceCache.cpp:1184` `SmBezierAux2d::BuildPolarBox` (dereferences a NULL `mBA_pSurface` on `BuildTreeTop` heads).
- **What happens:** Out-of-bounds or uninitialised reads, NULL dereference.
- **Trigger:** A line solve with a zero direction vector.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` or the line error.
