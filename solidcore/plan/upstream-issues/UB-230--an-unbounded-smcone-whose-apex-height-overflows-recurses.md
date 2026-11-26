#### UB-230. An unbounded `SmCone` whose apex height overflows recurses without bound while making its NURB
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:499` `SmCone::GetApex` (apex at height `-bot / tan(semi)`); `SmSurfOfRevolution.cpp:957` `SmSurfOfRevolution::MakeNurbWithSweepParams`; `SmLine.cpp:1679` (`SmLine::MakeNurb` asks the owner's `GetGenDirParamExtent`); `SmSurfOfRevolution.cpp:799` `GetGenDirParamExtent`.
- **What happens:** The apex parameter is NaN when the height overflows, so the generator's STEP interval is NaN. `MakeNurbWithSweepParams` re-adjusts the generator line (`AdjustSTEPInterval` never sees two equal NaN intervals), the line's `MakeNurb` asks the cone's `GetGenDirParamExtent`, which makes the cone's NURB again. The stack overflows. Related to UB-70 (same owner detour).
- **Trigger:** `SmCone::CreateCanonical` (semi-angle overload, no height) with `bot = 1e300`, semi `6e-11` deg (build, `fuzz01891`) or `1e-9` deg (iso circle, `fuzz00916`), or `bot = -1e300` with semi `5.7e-11` / `6e-11` deg (iso circle `fuzz00168`, inversion `fuzz00494`), all `surface.cone_p1`. The guard also covers the bot/top/height overload with `h <= 0` (the top radius is then the semi-angle of an unbounded cone); that path did not crash in the run.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.cone_p1`: `fatal runtime error: stack overflow`).
- **Confirmed:** yes.
- **Severity:** crash/UB (stack overflow).
- **Rust port:** No recursion. The `surface.cone_p1` FFI rejects unbounded cones whose `bot / tan(semi)` is not finite, except a zero semi-angle (`|semi| <= SM_EFF_ZERO`, a cylinder, which `GetApex` treats as having no apex).
