#### WR-156. The volume discontinuity unaligned check tests U three times
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10025-10027`.
- **What happens:** The rejection predicate repeats `m_bUnalignedU` and never checks V or W, so a V/W-only unaligned discontinuity can proceed into the aligned-plane algorithm.
- **Trigger:** a discontinuity report with U aligned and V or W unaligned.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no native execution of the unsupported report.
- **Rust port:** retains the U-only predicate, then rejects empty/malformed coordinate lists safely; custom discontinuity reports are outside the oracle's coverage.
