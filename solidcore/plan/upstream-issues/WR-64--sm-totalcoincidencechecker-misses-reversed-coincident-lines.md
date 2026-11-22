#### WR-64. `sm_TotalCoincidenceChecker` misses reversed coincident lines and returns `FALSE` as a status
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmCone.cpp:2501` and `:2508`, `sm_TotalCoincidenceChecker` (file static, def. `:2454`).
- **What happens:** For lines, the reversed test computes `dDist_E_S` from `sPt01` and `sPt11` (the end of this line and the start of the other, instead of `sPt10`), so reversed coincident lines are never found. The function then returns `FALSE` (0), which is not `SM_SUCCESS`; callers' `SER` turn it into an error return.
- **Trigger:** Lines `(0,0,0)-(1,2,3)` and `(1,2,3)-(0,0,0)`.
- **How found:** read during porting; compared by fixture `coinc_reversed_line` (`surface.cone_p1`, a verbatim copy of the static in the shim).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`SmStatus` 0 error).
