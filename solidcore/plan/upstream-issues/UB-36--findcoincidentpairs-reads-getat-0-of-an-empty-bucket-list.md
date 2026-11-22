#### UB-36. `FindCoincidentPairs` reads `GetAt(0)` of an empty bucket list
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmHashTable.h:2360`, `SmObjsInVoxels<TYPE>::FindCoincidentPairs`.
- **What happens:** An empty `pOptBucketIndices` is read at index 0.
- **Trigger:** `FindCoincidentPairs` with an empty, non-NULL bucket index array.
- **How found:** read during porting; PR #164 maintainer review reproduced the path through healer target catch-up.
- **Confirmed:** yes by the maintainer's release-C++ SIGSEGV in `FindCoincidentPairs <- Cache_CoinVertices <- HealTgts <- UpdateTargetLists` (PR #164). A box with cached edge/vertex/face-gap props, forged done op 6/8/9/16/26, no initial coincidence cache, and an existing vertex added to its targets reaches the empty incremental bucket list.
- **Severity:** crash/UB.
- **Rust port:** Checks no bucket. The healer FFI shim also refuses incremental catch-up with remaining old vertices and an uninitialized voxel extent. No prior vertices, a real initial coincidence cache, catch-up below step 6, and removal of all prior vertices remain accepted (direct FFI regressions in PR #164).
