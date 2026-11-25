#### WR-12. `SmObjsInVoxels::FindCoincidentPairs` misses pairs and changes the model
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `source/SMLib/inc/SmHashTable.h:2360`, `SmObjsInVoxels<TYPE>::FindCoincidentPairs`.
- **What happens:** `pBegPointItem` / `rBegPoint` are bound once per bucket and never advanced, so only head-vs-other pairs are found. `rEndPoint` is a reference, so `rEndPoint = pEndPointItem->m_sTgtPoint3d` overwrites stored points during the search.
- **Trigger:** Three coincident points in one voxel. The pair not involving the head is never reported.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
