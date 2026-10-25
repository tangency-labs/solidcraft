# primitive.primitive_creation_p2: planar-path union failures: an edge imprint that edits nothing need not stop the Boolean (2026-10-25)

Resolves "planar-path union failures: narrowed to an edge definition's proto vertices"
(cases `gen0003`, `gen0028`, `gen0039`, `gen0178`, `gen0200`, `gen0208`, `gen0252`).
With temporary `-Wl,--wrap` instrumentation of `SmEdgeDefinition::Imprint` and
`SmTopologyIntersector::IntersectInsertRelate` in the C++ shim, and a matching log in
the port's imprint loop, every case imprints the same edge definitions in the same
order with the same statuses on both sides: 3 or 4 succeed, then the last (Edge/Edge
sources, both ends on one proto vertex) returns `SM_ERR_NULL_POINTER (1006)` from
`GetFinalVertex` in both. So the intersector is not where they diverge. C++ removes
the edge definition, goes on (`SE_MSG`) and `IntersectInsertRelate` returns
`SM_SUCCESS` from `SetRelationships`. The port stopped there because it carries the
SolidCraft fix for UB-242 (tangency-labs/smlib af8db37), which ends the Boolean on any
imprint error.

UB-242's hazard is a B-rep left half edited by a failed `MakeEdgeInFace`. The failure here
comes from `SmEdgeDefinition::Imprint`'s `NER` checks on the four `GetFinalVertex`
results, a `const` lookup before any edit (`SmProtoTopology.cpp:1375-1386`), so both
B-reps stay consistent and upstream's continuation is defined. The port now stops only
when a failing imprint may have edited a B-rep: any vertex-definition or glue failure,
or an edge-definition failure at or after its first `MakeEdgeInFace`
(`ImprintOutcome::failure_edited`,
`ProtoTopologyManager::imprint_edge_definition_reporting_edits`). An edge definition
that stops before that point only warns, as upstream. The `boolean.topology_intersector_p2`
shim's copy of `IntersectInsertRelate` follows the same rule through a verbatim copy of
`ImprintIntersections`, which tests the same `const` lookups before each edge imprint.
All 7 cases now pass with values. Their known-mismatch entries are removed.

PR #884.
