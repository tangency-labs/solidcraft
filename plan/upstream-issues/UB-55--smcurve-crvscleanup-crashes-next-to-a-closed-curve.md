#### UB-55. `SmCurve::CrvsCleanup` crashes next to a closed curve
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `SmCurve.cpp:1106`, `SmCurve::CrvsCleanup` (`TrimGap` between the previous curve and a closed curve).
- **What happens:** Removing a tiny curve whose neighbour is closed (equal end points) segfaults inside `TrimGap`.
- **Trigger:** A cubic segment, then a tiny segment, then a closed quadratic loop.
- **How found:** crash in the C++ reference run (SIGSEGV; reproduced with the trigger above).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Completes the clean-up.
