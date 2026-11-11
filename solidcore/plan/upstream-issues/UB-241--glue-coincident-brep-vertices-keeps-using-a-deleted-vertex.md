#### UB-241. `SmProtoTopologyManager::GlueCoincidentBrepVertices` keeps using a vertex it has deleted
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:5854` `GlueCoincidentBrepVertices`, the common-edge loops at `:5999` (Brep 1) and `:6091` (Brep 2).
- **What happens:** For two coincident vertices joined by several common edges, the loop squeezes a short edge (`SqueezeEdge` deletes the second vertex) or glues the vertices (`GlueVertices` deletes it), records the vertex as deleted, and then goes on to the next edge. That step calls `GetOtherVertex`, `SqueezeEdge` or `GlueVertices` with the freed vertex.
- **Trigger:** a shell operation (ASan reports the use-after-free in `sm_RmFromSpatialTree`, through `SmBrep::Notify` from `GlueVertices`; tangency-labs/solidcore#371).
- **How found:** ASan run of the SolidCraft C API (tangency-labs/smlib `capi/`). Port and test by raykholo (tangency-labs/solidcore#406).
- **Confirmed:** yes (ASan, in the fork's C API tests); not reproduced in the solidcore difftest.
- **Severity:** crash/UB (use-after-free).
- **Fix:** tangency-labs/smlib da8d05e stops the loop once the vertex is recorded as deleted, in both B-reps' halves.
- **Rust port:** carries the fix (test `glue_stops_once_the_second_vertex_is_glued_away`, where two long common edges glued the deleted vertex a second time). Without it nothing is freed, but the later step glued, squeezed or deleted through a vertex that was already gone.
