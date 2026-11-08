#### WR-38. `SmBSplineSurface` data constructor zeroes the v knots when `s > r`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmBSplineSurface.cpp:87` (`smos_MemCpy(pNewSur->knv->U, V, (s+1) doubles, (r+1) doubles)`), constructor at `:63`.
- **What happens:** The v knots are copied with the u knot count as the destination size, so the copy is refused (status only `SE`'d) when `s > r`. The v knots stay 0 from calloc, and `sm_FixupKnotVector` then runs on zeros.
- **Trigger:** `SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)` with more v knots than u knots.
- **How found:** generated difftest case (every such `data` case).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it.
