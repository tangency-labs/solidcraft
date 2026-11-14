Updates: ND-2

#### ND-2 (update, 2026-10-30). `BooleanPostProcess` deletes topological vertices in hash order, so which split vertex survives varies
- **Severity:** Nondeterminism
- **Area:** Topology
- **Location:** `source/SMLib/src/SmMerge.cpp:2724-2726` (`m_sIntersectionVertices` filled from `m_vTI.GetMappedBrepObjects(SmVertex_TYPE, ..)`, `SmTopologyIntersector.cpp:945`, iterating the pointer-keyed `m_pOToB`); consumed by `SmMerge::BooleanPostProcess` (`SmMerge.cpp:1172`, `:1214`-`:1255`), whose `DeleteTopologicalVertex` merges the two edges at the first topological vertex visited.
- **What happens:** When the intersection splits one closed edge at two vertices that are both removable, only the first one visited is deleted. The merged edge then has a different parameter at the other vertex, so that vertex stays. Which vertex survives, and so its position, follows the hash order. This is the vertex counterpart of ND-258, and it changes geometry, not just loop starts.
- **Trigger:** `SmApiShellBrepFull` on `SmApiCreateCylinder((0,0,0), 5, 10)` at B-rep tolerance 1e-5, distance -1, extended, merged, offset solid, with face 1 (the bottom cap) open (tangency-labs/solidcore#1453). C++ visits [225, 135, 0 degrees] and keeps the 135-degree vertex (-2.8284215886295061, 2.8284294867261224, -1.5e-17). The port, in handle order, visits [0, 135, 225 degrees] and keeps the 225-degree vertex (-2.828433304880372, -2.8284192630706975, 4.9e-17).
- **How found:** root-causing plan/findings/offset.offset_executive_p2--issue-1453-cylinder-rim-split-vertex-differs.md (2026-10-30).
- **Confirmed:** yes. An out-of-tree instrumented copy of `SmMerge.cpp`/`SmBrep.cpp`, linked ahead of the reference library, showed the same split order on both sides (135 then 225 degrees) and the visit order above. The C++ order held under the default allocator and four `GLIBC_TUNABLES`/`MALLOC_*` settings, but it depends on addresses and rehash history, not on the input.
- **Rust port:** unchanged; the order is deterministic handle order (ND-2). The case is not compared in difftest.
