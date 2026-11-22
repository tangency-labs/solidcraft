#### UB-89. `SmVolume::ReadFromDB` fills NULL volumes and uses unread map types [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `SmVolume.cpp:8104`, `SmVolume::ReadFromDB` (the `SmVolume_TYPE` branch).
- **What happens:** (1) A next map of type `SmVolume_TYPE` recurses with `rpNewVolume->m_pNextMap` (NULL) as the volume to fill, and the branch writes `rpNewVolume->m_pNextMap` through it. A top-level `SmVolume_TYPE` read does the same with the caller's NULL. (2) `rDB.ReadType(lNextType)` / `ReadType(lOrientType)` statuses are ignored; at end of data the type stays uninitialised and is switched on. (3) The orient map read is cast to `SmTransform*` whatever its type, and a failed read leaves it NULL; `RefreshInvOrientMap` then copies, inverts and simplifies it (a NULL or mistyped object). The nested reads' statuses are ignored throughout.
- **Trigger:** An ASCII transform volume whose base data says `1` then `Object Type 22001` (next map), or `1` then `Object Type 22011` / `22004` (orient map), or ends right after a `1` flag.
- **How found:** read during porting.
- **Confirmed:** no. The harness's strict text grammar rejects these inputs.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Unsupported`, which the derived readers do not ignore. Nested reads are bounded by `DatabaseReader::enter_nested`.
