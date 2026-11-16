#### UB-178. One-list `SmStitch::StitchVertexPairs` glues a repeated vertex after freeing it
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmStitch.cpp:3016`-`3047` (coincidence scan), `:3060`-`3190` (glue loop), one-list `StitchVertexPairs`.
- **What happens:** The coincidence scan adds every later list entry within tolerance of `pV` to `sCoinVertices`, so a vertex listed twice lands there twice. Gluing the first copy (`GlueVertices(pV, copy)` or `SqueezeEdge`) deletes it; the second copy is then read (`IsLaminaVertex`, `GetPoint`) and glued after it was freed. Only `pV == sCoinVertices[jj]` (a repeat of `pV` itself) is caught.
- **Trigger:** A vertex repeated in `rVertexList` and coincident with an earlier, different vertex that keeps it as the vertex to delete.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection). The difftest FFI refuses repeated vertices. Not file-reachable: the only in-tree caller (`SmBrep.cpp:27427`) passes `pFace->GetVertices`, which has no repeats, so no [PSIRT] tag.
- **Severity:** crash / use after free.
- **Rust port:** Reproduces the order; a deleted vertex id makes the later query return `NotFound` instead of reading freed memory.
