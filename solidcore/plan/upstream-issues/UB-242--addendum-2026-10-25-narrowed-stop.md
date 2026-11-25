Updates: UB-242

#### UB-242 (update, 2026-10-25). The port's stop is narrowed to imprints that may have edited a B-rep
- **Location:** as UB-242; `SmEdgeDefinition::Imprint` (`source/SMLib/src/SmProtoTopology.cpp:1369`), whose `NER` checks on the four `GetFinalVertex` results (`:1375-1386`) return before any edit.
- **What happens:** An edge definition with no final vertex fails before `MakeEdgeInFace`, so it leaves both B-reps consistent. Upstream's continuation is defined in that case, and the UB-242 hazard (vertex uses freed by a failed `MakeEdgeInFace`) does not arise.
- **How found:** the 7 `primitive.primitive_creation_p2` planar-path sweep cases (`plan/findings/primitive.primitive_creation_p2--planar-path-union-imprint-continues.md`), traced with temporary `-Wl,--wrap` instrumentation.
- **Confirmed:** yes, on the C++ build: the same edge imprints return the same statuses on both sides, and C++ `IntersectInsertRelate` returns `SM_SUCCESS`.
- **Rust port:** `TopologyIntersector::intersect_insert_relate` still deletes the manager, records `"UB-242"` and returns the error when a failing vertex-definition imprint, glue, or edge-definition imprint at or after its first `MakeEdgeInFace` may have edited a B-rep (`ImprintOutcome::failure_edited`). Otherwise it goes on as upstream. The `boolean.topology_intersector_p2` shim follows the same rule.
