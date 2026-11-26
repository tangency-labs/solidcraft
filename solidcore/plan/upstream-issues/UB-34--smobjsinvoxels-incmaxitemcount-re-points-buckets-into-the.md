#### UB-34. `SmObjsInVoxels::IncMaxItemCount` re-points buckets into the array it is about to free
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/inc/SmHashTable.h:1803` (bucket items re-pointed at `&m_sPointItems.GetAt(id)`, which is still the old array) then `:1810` (`SetArray` frees the old array). Function `SmObjsInVoxels<TYPE>::IncMaxItemCount` (def. `:1747`).
- **What happens:** Once a model that still has items in buckets grows, every bucket item dangles, and `FindCoincidentPairs` reads freed memory. Upstream comments at `SmHashTable.h:2113` and `SmHealData.cpp:3126` describe the hazard and reserve capacity up front. The growth path itself is unchanged.
- **Trigger:** `SmObjsInVoxels::Add` beyond the reserved size (`size + 7 > GetDataSize()`) while items are in buckets, then `FindCoincidentPairs`.
- **How found:** read during porting.
- **Confirmed:** no. The shim refuses these programs.
- **Severity:** crash/UB (use after free).
- **Rust port:** Stores indices, not pointers.
