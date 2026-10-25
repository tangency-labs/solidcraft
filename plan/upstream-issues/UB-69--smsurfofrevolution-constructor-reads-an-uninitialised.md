#### UB-69. `SmSurfOfRevolution` constructor reads an uninitialised `bOnPlane`
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfRevolution.cpp:147`-`149` (constructor, def. `:94`); `SmBSplineCurve.cpp:5433` `SmBSplineCurve::IsOnPlane`; `SmGeomUtility.cpp:3829` `smgu_PlanePointDistance`.
- **What happens:** `IsOnPlane` fails before writing `bOnPlane` when `smgu_PlanePointDistance` rejects the normal (`|y|^2 - 1 > SM_EFF_ZERO`). The constructor only `SE`s the status, then reads `bOnPlane`.
- **Trigger:** A planar non-line generator with a placement Y axis of length 1.1.
- **How found:** read during porting (PR #15 review).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Accepts only unit placement axes for the constructor.
