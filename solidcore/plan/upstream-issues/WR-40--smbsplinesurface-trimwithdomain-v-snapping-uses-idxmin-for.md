#### WR-40. `SmBSplineSurface::TrimWithDomain` v snapping uses `idxMin` for both boundaries
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmBSplineSurfaceNLib.cpp:7455`, `SmBSplineSurface::TrimWithDomain`.
- **What happens:** The v loop's upper-boundary branch writes `idxMin`, so `idxMax` stays `-1` and `sm_CheckTrimBoundaries` never widens `vr`. The u loop's "slightly above the knot" branch does not set `idxMax`. Only the u boundaries are checked for being too close.
- **Trigger:** A v trim boundary just above an interior knot.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
