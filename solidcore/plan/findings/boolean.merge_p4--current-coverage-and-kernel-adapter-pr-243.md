# boolean.merge_p4 — current coverage and kernel adapter (PR #243)

**No non-null case compares yet; Boolean success paths compared: zero.**
Every valid non-null operand pair stops at merge_p1's CalculateBoundingBox
prerequisite (`query.brep`) before mutation. The 30 matches cover null-operand
errors across all eleven operations. Status is data, so both sides also compare
doing-Boolean/editing flags, intersection/component counts and full surviving
graphs. Error and success records share the snapshot path. Native post-state is
imported only on the C++ side; Rust serializes its own arenas through the shared
p10 serializer. Integer topology is exact, geometry uses 1e-9 relative tolerance,
and there are no absolute floors. These values do not certify the later Boolean
tail; its evidence remains source review and focused adapter regressions.

The adapter calls merge_p1/p5 using one canonical MergeState<Relations>, caller
attributes and notification records. Common lists share the generic intersector
implementation. Region callbacks resolve actual source lists and operand records.
The temporary inverse token spans the entire operation, including errors, imprint,
component extraction and operand deletion. Editing flags and callback ownership
are restored on Result exits. Split-region source handles are captured before
mutation, including aliasing children. The shim frees unowned planes on rejection
and exports every component graph. Current geometry families avoid closed edges;
adding closed curves requires reviewing UB-175. Shared FFI reader bounds are 1e100.

After batch #259, the real intersector_p2 IntersectInsertRelate,
RemoveRelationship, FixGaps and DeleteFaces methods are wired. Relations moves its
actual projections through the generic intersector, restoring both after errors;
there is never a second live mate store. The arena adapter borrows both B-reps,
attributes and notification records, and retains curve handles/proto state across
calls. It delegates geometry operations to their existing ports. Cache evaluation
uses a read-only snapshot of each operand's own settings, not default tolerances.
Notification callbacks are temporarily owned by a notification Context while the
live kernel retains its marks/tolerances; every Result exit restores callbacks.
Distinct other-context callbacks are passed directly. Object identity reserves a
27-bit namespace per operand pair, with separate slots for every topology class.
Explicit intersection tolerances are forwarded; automatic merge-constructor
selection remains outside this unit.

Transform calls brep_p5 with the real notification environment;
FindAndSetInfiniteRegion calls brep_p5; OrientSheetSurfaces calls brep_p9. Main's
expanded vertex-copy and CopyFaces notification interfaces use the same records
and caller attributes. These calls no longer stop at a p4 stub of a ported method. Their existing
nested boundaries remain in the owning units: p5 FindAndSetInfiniteRegion still
has its FindInfiniteRegion (p9) adapter boundary, while nonidentity Transform
reaches Notify (brep_p2), and sheet orientation reaches IsSheet (brep_p2).
IntersectInsertRelate reaches intersector_p2's existing GetFaces boundary, already
assigned to sc-topo_isect_p2's follow-up in COORD. That shared boundary is not
reimplemented here.

CloseIntersectionLoop remains a callgraph gap: units.json lists it unused, but
SmMerge.cpp:2958 calls SmTopologyIntersector.cpp:6108-6354. Per overseer direction
its proposed owner is boolean.topology_intersector_p2. Batch #259/#203 was checked
and does not contain it; that partial owner's own missing part remains stubbed,
pending the overseer's follow-up. The complete retry/cleanup tail follows the call.

Remaining direct p4 tags:

| Owner | Status after batch #259 | Callee |
| --- | --- | --- |
| boolean.topology_intersector_p2 | partial; own missing part | CloseIntersectionLoop (proposed owner above) |
| boolean.merge_p2 | todo | MakeFaceuse |
| query.brep | todo | Point3DClassify; merge_p1 CalculateBoundingBox prerequisite |
| topology.brep_p2 | todo | IsSheet, IsOrientedSheet, DeleteStaleCaches |
| topology.brep_p3 | todo | component constructor including infinite region |

Point3DClassify replaces only its call; the PC_REGION logic remains in the caller.
No dependency algorithm is copied and there are no incoming boolean.merge_p4 stubs.
The three review-requested mutant-catching assertions remain: asymmetric coincidence
maps, the other-to-primary region map, and IsVoid == 1. Additional integration
regressions cover relationship removal/failure restoration, empty FixGaps/DeleteFaces,
the real IntersectInsertRelate deferral, identity/nonidentity transform boundaries,
notification identities and both operand callbacks with locked marks after error.

Verification after batch #259: running on sc-builder-5; final summaries pending.

Update (PR #281): CloseIntersectionLoop is now co-ported into
boolean.topology_intersector_p2 (`TopologyIntersector::close_intersection_loop`), and
`KernelManifoldIntersector::close_intersection_loop` calls it over the `ArenaPair`.
The `ManifoldIntersector` method now takes the caller `AttributeStore` and has no
default, so the table row above is historical. `ArenaPair` also gains
`edge_normalized_evaluate`, the `IntersectorBrepP2` method this co-port adds
(`brep_faces`, `brep_combine_coincident_vertices` and `fix_gaps(k, ..)` came with
#274's integration in batch #305). All valid merge_p4 operand cases still stop first at merge_p1's
`temporary_brep_transform` (`query.brep`), so no merge_p4 case reaches the loop yet.
