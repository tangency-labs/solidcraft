#### UB-254. `SmFillet3x2MixedConvexityCorner::CreateCornerPatch` uses a freed fillet edge listed twice in `m_vEdges`
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletCorner.cpp:4692-4700` (`SmFillet3x2MixedConvexityCorner::CreateCornerPatch`, def. `:4623`): `m_vEdges.RemoveAt(lFoundIndex)` removes one entry of the on-edge fillet edge, then `delete pFEOnEdge`; the base `CreateCornerPatch` and `MakeFaceBrep` that follow walk `m_vEdges`.
- **What happens:** When the corner's edge list holds the on-edge edge more than once, the remaining entry points at the deleted edge, and the patch dereferences it.
- **Trigger:** a 3x2 corner whose `m_vEdges` repeats its `SM_FE_ON_EDGE` edge (the corner's own topology code never builds one; the difftest model can).
- **How found:** `difftest --unit blend.fillet_corner_p3 --fuzz 500` (PR #722), SIGSEGV in the shim.
- **Confirmed:** yes (C++ build: signal 11 in fuzz00157).
- **Severity:** crash/UB (use after free) for such an input.
- **Rust port:** not affected (the edge is an arena id; the deleted edge is reported as not found). The shim's pre-check (`blend_fillet_corner_p3_static.inc`) refuses the input.
