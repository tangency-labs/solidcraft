Updates: WR-452

#### WR-452 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

The defect is still in v1.0.1. `SmEdgeDefinition::Imprint` moved from `SmProtoTopology.cpp:1369` to `:1375` and still passes `m_pXSectInterval` and the curves to `MakeEdgeInFace` (`:1462`, `:1541`) unchanged; its only new lines (`:1468`-`:1471`) forget a curve that a failed `MakeEdgeInFace` already implanted. The one change nearby is in `SmEdgeDefinition::IsInteriorPoint` (`:1343`-`:1348`): it now returns FALSE when the drop distance is not within the tolerance. Its callers (`:5148`, `:5154`, `:5199`, `:5205`) trim an edge definition to a split point; the record's trim moves an end to the proto vertex inside the interval, which this check does not do.

Rust: on `main` with the v1.0.1 port, `issue_435_m44x1_edges_end_at_their_vertices` and `issue_435_m33x3_5_edges_end_at_their_vertices` still pass: the gap exceeds half the vertex tolerance with `imprint_trims_edge_to_proto_vertex` off and is below 2e-6 with it on. The default path agrees with the C++ 1.0.1 in difftest (`topology.proto_topology_p1`, `_p2` and `_p3`: 1922, 1767 and 3013 cases, all pass, max_rel_err 0). **Confirmed:** still no, as in the record: the C++ 1.0.1 was not run on the thread-groove inputs.
