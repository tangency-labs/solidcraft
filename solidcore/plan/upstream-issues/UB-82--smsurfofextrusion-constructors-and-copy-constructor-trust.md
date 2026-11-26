#### UB-82. `SmSurfOfExtrusion` constructors and copy constructor trust failed calls
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfExtrusion.cpp:104`-`109` and `:204`-`209` (constructors: `SE(IsOnPlane(..))`, then read `bOnPlane`); `:70`, `:162`, `:170` (`SE` of the generator evaluations whose points are then used); `:252`-`:259` (copy constructor: `SE(SM_ERR)` for a NULL or non-B-spline generator copy, then `m_pGenCurve->SetOwner(this)`).
- **What happens:** A failed `IsOnPlane` leaves `bOnPlane` uninitialised; a failed evaluation leaves the tolerance point or origin unset; copying a surface without generator dereferences NULL.
- **Trigger:** Copying the empty I/O surface; a generator whose evaluation fails.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns the error (`SM_ERR_NULL_POINTER` for the copy).
