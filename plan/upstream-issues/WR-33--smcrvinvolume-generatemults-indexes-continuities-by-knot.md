#### WR-33. `SmCrvInVolume::GenerateMults` indexes continuities by knot index
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmCrvInVolume.cpp:968`, `SmCrvInVolume::GenerateMults` (def. `:911`).
- **What happens:** `m_sConts[ii]` uses the knot index `ii`, not the matched break index, so `GetKnots`' cached path reports the wrong breaks' multiplicities. The cached and computed paths also disagree (4 vs 3 at the ends).
- **Trigger:** `GetKnots` with an interval on a cached curve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
