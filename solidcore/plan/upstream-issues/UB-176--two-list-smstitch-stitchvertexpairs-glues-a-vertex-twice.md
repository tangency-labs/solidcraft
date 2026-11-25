#### UB-176. Two-list `SmStitch::StitchVertexPairs` glues a vertex twice
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SMLib/src/SmStitch.cpp:3242`-`3749` (the two-list `StitchVertexPairs`): each glue deletes `sUsingVerts2[...]` and nulls only its own slot.
- **What happens:** A vertex repeated in `rVertexList2`, or present in both lists, is glued (and deleted) once per occurrence, so a later `GlueVertices` gets an already-deleted vertex (double free / use after free). The sorting loops also index `sSortedIndex1` / `sSortedIndex2` with the unfiltered counts, reading past them when a list holds `NULL` entries.
- **Trigger:** caller-built lists with a repeated vertex, a vertex in both lists, or a `NULL` entry. `SmMerge::MergeAddedTopology` (`source/SMLib/src/SmMerge.cpp:5735`) reaches it with a vertex repeated in its caller's `pMergeVerts`, which it copies into `rVertexList2` unchecked: e.g. the ASCII `plane_square.smb` with vertices `[0, 0]`, no faces or edges, tolerance 10 (stitch tolerance 40).
- **How found:** source review while porting `heal.stitch_p1` (PR #196); the difftest FFI refuses repeats, shared vertices and `NULL`s. The `MergeAddedTopology` route: an adversarial `boolean.merge_p2` fixture run with that unit's duplicate guard relaxed (PR #229).
- **Confirmed:** yes (C++ reference build, through `MergeAddedTopology`: segfault, exit 139; the same request without the repeated vertex succeeds).
- **Severity:** crash/UB.
- **Rust port:** `heal::stitch::p1::Stitch::stitch_vertex_pairs` returns `InvalidInput` where the sorted index arrays would be read past; repeated vertices are a caller error the FFI refuses. `boolean.merge_p2`'s FFI refuses any entity listed twice in a `MergeAddedTopology` list.
