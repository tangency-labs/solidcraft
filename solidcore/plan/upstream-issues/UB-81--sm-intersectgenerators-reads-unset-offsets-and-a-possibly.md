#### UB-81. `sm_IntersectGenerators` reads unset offsets and a possibly NULL curve list
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfExtrusion.cpp:2978`-`2986` (`sm_IntersectGenerators`, def. `:2658`; arrays declared `:2725`-`:2726`).
- **What happens:** For a range solution, `pOpt3DCurves->GetSize()` is read before the `pOpt3DCurves != NULL` test, and `pOffsetCurve[jj][k]` / `dOffsetLength[jj][k]` are compared although they are only set when an end of the overlap interval equals one of four bounds exactly (`==` on doubles) and the end overlaps at all.
- **Trigger:** A range solution of the projected generator intersection whose overlap ends match none of the four bounds, or a caller passing `NULL` curves.
- **How found:** read during porting.
- **Confirmed:** no (its projected solve, `SmCurve::GlobalCurveSolve`, is not ported yet, so no range case is compared).
- **Severity:** crash/UB.
- **Rust port:** A missing curve list has size 0; unset offsets compare unequal (the case falls to "too complicated").
