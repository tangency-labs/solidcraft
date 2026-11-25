#### DOC-3. "Small topology" tolerances are 1000x larger, not tighter
- **Severity:** Docs
- **Area:**
- **Location:** `SmTol.cpp:661` (also `:806`, `:892`), `SmTol::GetZoneTol3d`.
- **What happens:** `GetZoneTol3d(SM_ST_SMALL_SIZE)` returns `s_LargeSmallSizeRatio * s_ZoneTol3d` (default 1000x), despite comments that promise tighter tolerances. The `const SmContext*` overloads ignore the small flag.
- **Trigger:** `GetZoneTol3d(SM_ST_SMALL_SIZE)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code.
