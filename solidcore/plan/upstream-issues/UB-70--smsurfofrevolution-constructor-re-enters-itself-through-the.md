#### UB-70. `SmSurfOfRevolution` constructor re-enters itself through the generator
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfRevolution.cpp:94` (constructor); `:202` `RebuildPolarConverter`.
- **What happens:** The constructor sets the generator's owner, then evaluates it. A fresh `SmLine` generator with no NURB runs `MakeNurb`, which asks the owner for `GetGenDirParamExtent`. That makes the surface's NURB through the virtual `MakeNurb` from inside the constructor. Separately, `RebuildPolarConverter` dereferences a NULL iso curve when `CreateIsoParametricCurve` fails (only asserted).
- **Trigger:** `SmSurfOfRevolution` built on an `SmLine` from its two-point constructor.
- **How found:** read during porting.
- **Confirmed:** no. The shim makes the line's NURB first.
- **Severity:** crash/UB.
- **Rust port:** Evaluates the generator without the owner detour, and returns the error.
