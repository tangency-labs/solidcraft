#### HANG-10. `SmCurve::GlobalCoincidenceChecker` recurses without a depth bound
- **Severity:** Hang
- **Area:**
- **Location:** `SmCurve.cpp:19950`, `SmCurve::GlobalCoincidenceChecker`.
- **What happens:** It recurses on closed and half-closed curves with no limit. The recursive calls also force `bSnapToEnds = TRUE`.
- **Trigger:** Two coincident closed curves.
- **How found:** read during porting.
- **Confirmed:** no. Unbounded recursion was not observed.
- **Severity:** hang (potential stack exhaustion).
- **Rust port:** Stops at 200 nested splits with `InvalidInput`.
