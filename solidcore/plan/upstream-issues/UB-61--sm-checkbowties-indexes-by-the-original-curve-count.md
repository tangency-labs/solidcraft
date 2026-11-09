#### UB-61. `sm_CheckBowTies` indexes by the original curve count
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCompositeCurve.cpp:2352`, `sm_CheckBowTies`.
- **What happens:** Reads `crOffsets[0]`, `crOffsets[lNumOrig-1]` and `crOrigCurves[0]` without checks. `IntersectAndTrimOffsets` ignores its status.
- **Trigger:** One-sided offsetting that drops degenerate offsets, leaving fewer raw offsets than original curves.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` and goes on without bow-tie points.
