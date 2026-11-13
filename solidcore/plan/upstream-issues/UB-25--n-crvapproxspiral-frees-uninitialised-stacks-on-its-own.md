#### UB-25. `N_CrvApproxSpiral` frees uninitialised stacks on its own input errors
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_Spiral.cpp:108`, `:111`, `:120`, `:123` (`NL_ERROR` checks) run before `N_InitNurbs(&SL)` at `:148`. The `EXIT` path calls `N_EndNurbs(&SL)`. Function `N_CrvApproxSpiral` (def. `:88`).
- **What happens:** Invalid input jumps to `EXIT`, which frees uninitialised local stacks. `SmBSplineCurve::CreateHelixSegment` (`SmBSplineCurveNLib.cpp:539`) validates first, but its checks let some of these inputs through: height in `[-SM_EFF_ZERO, NL_MTOL)`, both radii below `NL_MTOL`, turns in `[SM_EFF_ZERO, NL_MTOL)`.
- **Trigger:** `height = 0`, `r0 = r1 = 2`, `turns = 5`.
- **How found:** crash in the C++ reference run (`free(): invalid pointer`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `NlibCode::Input` (helix: `SM_ERR`).
