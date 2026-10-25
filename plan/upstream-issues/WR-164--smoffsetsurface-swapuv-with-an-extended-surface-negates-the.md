#### WR-164. `SmOffsetSurface::SwapUV` with an extended surface negates the distance twice
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1767` and `:1772`, `SmOffsetSurface::SwapUV`.
- **What happens:** Swapping the parameters flips the normal, so the distance is negated; with an extended surface it is negated once for it and once for the base, leaving it unchanged and the offset on the wrong side.
- **Trigger:** `SwapUV` on an owning offset that has stored an extension.
- **How found:** read during porting.
- **Confirmed:** yes (fixture `extension_sequence`).
- **Severity:** wrong result.
- **Rust port:** Kept.
