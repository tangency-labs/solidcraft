#### WR-99. `SmSurface::LocalSurfaceIntersect` retries maximum boundaries after a found boundary solution
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SMLib/src/SmSurface.cpp:27782`, `:27804`, `:27826` (`SmSurface::LocalSurfaceIntersect`, step 4: the boundary curve solves at the point where the Newton solve stopped out of bounds).
- **What happens:** The conditions read `(!bFoundSolution && SM_ARE_SAME(x, min)) || SM_ARE_SAME(x, max)`: the `!bFoundSolution` guard binds to the minimum test only, so a solution found on an earlier boundary is solved again (and replaced, or lost when that solve fails) whenever the stop point is also on a later maximum boundary.
- **Trigger:** `bDoBoundaryTesting`, a Newton solve stopped by the boundary handler at a corner of the domain whose second side is a maximum `v` (or the other surface's maximum).
- **How found:** read during porting.
- **Confirmed:** no (the boundary curve solve, `sm_IntersectSrfBndryCrv`, is unit `surface.surface_p8`; those cases defer).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.
