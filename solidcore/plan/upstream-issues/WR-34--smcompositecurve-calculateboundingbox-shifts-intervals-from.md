#### WR-34. `SmCompositeCurve::CalculateBoundingBox` shifts intervals from 0
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmCompositeCurve.cpp:392`, `SmCompositeCurve::CalculateBoundingBox`.
- **What happens:** Segment intervals are shifted from `0`, not from the first segment's start (as `GetNaturalInterval` does), and the shifted interval is passed to each segment.
- **Trigger:** A composite whose first segment starts at `t = 5`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
