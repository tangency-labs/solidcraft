#### WR-170. `IntersectTopology` leaks the copies of registered intersection curves; a failed `IntersectInsertRelate` leaks its manager
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:1781-1792` (and the deleters built only at :1874), `IntersectTopology`; :1161, :1188, :1212, `IntersectInsertRelate`.
- **What happens:** For a face pair registered in the intersection table, `CheckIfHaveExistingSSI` returns fresh copies of the registered curves and the loop `continue`s before the `SmObjsDelete` guards that would delete them, so every registered pair leaks its copies (once per call). `IntersectInsertRelate` allocates `m_pProtoTopoMgr` and returns through `SER_MSG` when `IntersectTopology` or `ResolveIntersections` fails, leaving the manager allocated and installed; the next call overwrites the pointer.
- **Trigger:** `RegisterExistingSSI` with curves for a face pair of the two B-reps, then `IntersectInsertRelate`; any failing `IntersectTopology` (e.g. a failing `CoincidenceCheck` or `FaceIntersect`).
- **How found:** read during porting (`boolean.topology_intersector_p2`, PR #203).
- **Confirmed:** no (source inspection).
- **Severity:** memory leak; results unaffected.
- **Rust port:** Faithful: the copies stay unreferenced in the caller's curve store and are not reported as deleted; after a failure the manager stays installed on the intersector.
