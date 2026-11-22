#### WR-52. `SER` inside `SmBoolean` functions reports a failure as `TRUE`
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:4195` `sm_IsPointOnCurve` and `:7376`, `:7391` `SmSurfaceIntersector::IsPointOnCurve`.
- **What happens:** `SER(status)` returns the non-zero `SmStatus` from a function returning `SmBoolean`, so a failing point solve, curve evaluation or curve drop makes the point count as "on the curve".
- **Trigger:** A curve whose `EvaluatePoint` or `DropPoint` fails, or a failing Hermite point solve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Faithful (`Ok(true)` on such an error).
