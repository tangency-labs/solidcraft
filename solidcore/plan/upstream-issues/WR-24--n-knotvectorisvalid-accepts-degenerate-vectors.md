#### WR-24. `N_KnotVectorIsValid` accepts degenerate vectors
- **Severity:** Wrong result
- **Area:** NLib
- **Location:** `NL_BasisBasic.cpp:172`, `N_KnotVectorIsValid`.
- **What happens:** Accepts `p+1` equal knots (e.g. `[2,2,2,2]` with `p = 3`), and a single knot with `p = 0`. Later routines index such vectors as if they had a span.
- **Trigger:** `[2,2,2,2]`, `p = 3`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (validation gap).
- **Rust port:** Accepts them too (faithful).
