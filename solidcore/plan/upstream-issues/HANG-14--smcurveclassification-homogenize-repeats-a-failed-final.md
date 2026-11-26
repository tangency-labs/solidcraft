#### HANG-14. `SmCurveClassification::Homogenize` repeats a failed final squeeze forever
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmCurveClass.cpp:6520-6530` (`Homogenize`), `:3715-3741` (`sm_SqueezeOutSmallestInterval`).
- **What happens:** The two final size-equalization loops ignore the squeeze status. If `ConnectIntervals` fails without removing an interval, the size difference remains and the same loop repeats indefinitely.
- **Trigger:** Unequal classification sizes after the earlier homogenization heuristics, with a smallest interval that `ConnectIntervals` cannot remove.
- **How found:** maintainer review of PR #138 and source inspection.
- **Confirmed:** source path only; no native hang reproduction was run.
- **Severity:** hang.
- **Rust port:** Ordinary squeeze statuses are ignored through `ignore_status`, preserving `Unported` propagation; a documented no-progress check returns an error if the interval count did not decrease.
