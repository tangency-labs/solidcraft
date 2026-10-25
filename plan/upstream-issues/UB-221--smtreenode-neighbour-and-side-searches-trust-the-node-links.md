#### UB-221. `SmTreeNode` neighbour and side searches trust the node links
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/src/SmTree.cpp:3027`-`3236` `SmTreeNode::GetLeftNeighbors` / `GetRightNeighbors` / `GetBotNeighbors` / `GetTopNeighbors` (recurse up through `m_pParent` and into `m_pParent->m_pChild1/2`), `:3244`-`3510` `GetRightSide` and the other `Get*Side` searches (recurse into `m_pChild1` and `m_pChild2`), `:2749` `PropagateToParents` (reads the sibling `m_pParent->m_pChild1/2` and its `m_pData`).
- **What happens:** No null, consistency or cycle checks. A node with one child, a parent that does not list the node, a child whose parent pointer names another node, or cyclic parent links give a NULL dereference (SIGSEGV) or unbounded recursion (stack overflow).
- **Trigger:** A subdivision tree whose node links disagree, e.g. the `geom.tree` subdivision program with one node duplicated (`/nodes element 5 duplicated`, n2000 `fuzz00296`) or a child link cleared on one side only (`/nodes/28/c1 = -1`, `fuzz01815`). Trees built by upstream's own subdivision always have two distinct children and matching parent links.
- **How found:** crash in the C++ reference run (`difftest --unit geom.tree --fuzz 2000`: 14 cases, 11 SIGSEGV, 3 stack overflows; `fuzz-out/n2000/geom.tree/`).
- **Confirmed:** yes.
- **Severity:** crash/UB. Not file-reachable (needs hand-built or corrupted `SmTreeNode` links).
- **Rust port:** Arena ids; a missing node is `InvalidInput`. The `geom.tree` FFI (`well_formed`, `crates/solidcore-ffi/src/units/geom_tree.rs:152`) now rejects such link tables before calling C++.
