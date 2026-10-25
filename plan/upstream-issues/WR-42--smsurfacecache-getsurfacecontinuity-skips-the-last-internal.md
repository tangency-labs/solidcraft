#### WR-42. `SmSurfaceCache::GetSurfaceContinuity` skips the last internal v knot
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmSurfaceCache.cpp:1711` (`iv+1<rVConts.GetSize()-1`), `SmSurfaceCache::GetSurfaceContinuity` (def. `:1672`).
- **What happens:** The v loop stops one earlier than the u loop, so the continuity at the last internal v knot is never checked.
- **Trigger:** A surface whose only C0 v knot is the last internal one.
- **How found:** generated difftest case.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
