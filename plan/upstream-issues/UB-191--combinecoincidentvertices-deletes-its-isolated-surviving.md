#### UB-191. `CombineCoincidentVertices` deletes its isolated surviving vertex when inputs alias
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp:1354-1366`, `SmBrep::CombineCoincidentVertices`.
- **What happens:** The zero-incident-edge branch notifies, sets the surviving vertex's point/tolerance, then calls `DeleteVertex(pVertexToDelete)` and returns `SM_SUCCESS`. If both input pointers are the same, the supposedly surviving vertex is deleted too. Nothing inside the function touches it afterwards, so the call itself is defined; a caller that keeps using the survivor pointer reads freed memory.
- **Trigger:** Pass the same isolated vertex as both `pSurvivingVertex` and `pVertexToDelete`. Aliases with incident edges go through `GlueVertices(v, v)` and keep the survivor.
- **How found:** PR #194 review and source inspection while porting `topology.brep_p5`; corrected in the second review round (the function was first thought undefined).
- **Confirmed:** no native reproduction of the caller hazard. The whole call is compared in difftest; only the shim's target-stage op, which reads the survivor afterwards, refuses this input.
- **Severity:** dangling caller-held survivor (caller use-after-free).
- **Rust port:** Ported as upstream: the isolated aliased combination deletes the vertex and succeeds. Rust ids cannot dangle into freed memory; a later lookup of the deleted id returns `NotFound`.
