#### UB-132. `SmProtoTopologyManager::ResolveOverlappingProtoEdges` reads a deleted proto edge when an edge is listed twice in the manager
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:7032`-`7035` (`ResolveOverlappingProtoEdges`, the `rPEs[ii]` loop whose index repeats after a group was removed) with `source/SMLib/inc/SmProtoTopology.h:798` (`AddProtoEdge` does not de-duplicate) and `SmProtoTopology.cpp:4346` (`RemoveProtoEdge` removes the first copy only).
- **What happens:** Removing an overlap group deletes its proto edges and `RemoveProtoEdge` drops only the first entry of each from `m_sPEs`; a second entry of the same pointer stays in the list, so the repeated-index loop calls `GetOverlappingPEGroup` on the deleted edge (use after free).
- **Trigger:** two proto edges overlapping each other symmetrically (0 and 1) where one of them was added to the manager twice (`AddProtoEdge` repeated), then `ResolveOverlappingProtoEdges`. The C++ reference run exits with signal 11.
- **How found:** adversarial review of the difftest guard (the maintainer's crashing script).
- **Confirmed:** yes (the script was run on the C++ reference; the case `edge_proto_p3_rejected_duplicate_manager_entry_0`/`_1` now refuses it on both sides).
- **Severity:** crash/UB.
- **Rust port:** proto edges are arena ids that are never freed, so the repeat reads a deleted-edge slot and finds no group; nothing is read after free.
