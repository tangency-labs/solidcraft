#### UB-118. `SmProtoEdge::Resolve` / `SmProtoVertex::Resolve` dereference null and deleted definitions
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3440` and `:3454`, `SmProtoEdge::Resolve` (def. `:3391`); `:2338`/`:2356`, `SmProtoVertex::Resolve` (def. `:2278`).
- **What happens:** For a proto edge from a proto vertex to itself whose curve is longer than twice the tolerance with ends farther apart, `m_pStartPV` is dereferenced although both ends may be `NULL`, and `pVD0` (`GetClosestVertexDefinition`, `NULL` for a proto vertex without definitions) is dereferenced. In both resolves a definition listed twice is compared with itself (`CanCombine...` picks the second) and deleted, leaving the other entry dangling for the next comparisons; two deleted edge definitions sharing a curve free it twice (`Destruct`), and a deleted curve still used by a kept definition is used after free.
- **Trigger:** a closed proto edge on a proto vertex without vertex definitions; a duplicated entry in the definitions list (e.g. `CombineProtoEdge` of edges sharing a definition).
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim replays both loops and refuses those calls).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput` for the null proto vertex and definition; definitions are ids (no double delete).
