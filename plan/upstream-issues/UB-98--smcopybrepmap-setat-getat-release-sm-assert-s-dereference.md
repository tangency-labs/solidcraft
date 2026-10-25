#### UB-98. `SmCopyBrepMap::SetAt` / `GetAt` release `SM_ASSERT`s dereference null objects
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCopyBrepMap.cpp:21` `SetAt`, `:48` `GetAt`; the release `SM_ASSERT(a)` is `if(a) {}` (`source/SMLib/inc/SmMessages.h:714`), so its expression is still evaluated.
- **What happens:** `SetAt` evaluates `pCopyObj->GetType() == pOrigObj->GetType()`, a virtual call through a null pointer when either object is null. `GetAt` does the same with the original it looks up, which is null for an object the map has no entry for.
- **Trigger:** `SetAt(NULL, p)`, `SetAt(p, NULL)`, or `GetAt(p)` for a `p` never passed to `SetAt`.
- **How found:** crash in the C++ reference run (`topology.proto_topology_p1` list scripts).
- **Confirmed:** yes (segfault in `SmCopyBrepMap::SetAt`).
- **Severity:** crash/UB.
- **Rust port:** `CopyBrepMap::set_at` takes non-null objects; `get_at` of an object without an entry is `InvalidInput`. The shim refuses both.
