#### WR-16. `SmTopology::Notify` merge uses the deleted object as the source
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `SmTopology.cpp:220` (and `:239`), `SmTopology::Notify` (def. `:187`).
- **What happens:** `SM_NO_MERGE_IN_BREP` takes `pData2` (the deleted object) as the source. Upstream marks this itself: "GWC:TODO try typo bug fix". Merges also OR the target flags into both targets but copy none from the source.
- **Trigger:** Merge notify of two topology objects with different flags.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
