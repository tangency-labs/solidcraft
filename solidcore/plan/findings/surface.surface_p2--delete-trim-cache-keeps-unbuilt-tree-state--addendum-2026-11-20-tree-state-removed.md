# surface.surface_p2: `delete_trim_cache` and the tree state; the state is gone (2026-11-20)

Updates: plan/findings/surface.surface_p2--delete-trim-cache-keeps-unbuilt-tree-state.md

- The open question is moot: `CacheTreeState` no longer exists. The surface's cache slot keeps only its trimmed cache (`TrimCacheSlot`).
- A new trimmed cache is built unless the context is doing a Boolean operation (`trim_srf_cache_tree`). This holds whether or not an earlier cache of the surface was made, kept or deleted.
- After `delete_trim_cache`, the next `smsurf_GetSurfaceCache` outside a Boolean operation therefore builds its tree. This is what the C++ shim's `DeleteObjectsCache` gave in `query.face_p1` gen0342, gen0625 and gen0890.
- The difftest runners' reset to `NoCache` is gone with the state. `query_face_p1::drop_trim_cache` only deletes the trimmed cache, and the unit's 2266 cases pass unchanged.
- The second observation (the leased cache's tree during the global solve) is not affected.
