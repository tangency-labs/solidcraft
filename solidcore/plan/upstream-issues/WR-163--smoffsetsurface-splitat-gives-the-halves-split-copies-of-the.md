#### WR-163. `SmOffsetSurface::SplitAt` gives the halves split copies of the base as extended surfaces
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1682`, `SmOffsetSurface::SplitAt`.
- **What happens:** With an extended surface, the halves' `m_pExtendedSurface` come from splitting `m_pSurface` again instead of `m_pExtendedSurface`, so the extension is lost.
- **Trigger:** `SplitAt` on an offset that has stored an extension.
- **How found:** read during porting.
- **Confirmed:** no (fixture `extension_then_split` waits on `surface.surface_p3`).
- **Severity:** wrong result.
- **Rust port:** Kept.
