#### WR-32. `SmCircle::CreateSimpleOffset` never shrinks for the anti-parallel normal
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmCircle.cpp:339`-`340`, `SmCircle::CreateSimpleOffset` (def. `:321`).
- **What happens:** `IsParallelTo(+Z)` also accepts anti-parallel vectors, so the `-Z` branch (radius minus distance) is dead.
- **Trigger:** Offset with normal `-Z`: the radius grows.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
