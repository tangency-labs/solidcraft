The oracle calls the real `SmMerge::ManifoldBoolean`, with independently built
native B-reps. The four procedural operands are an empty B-rep, an isolated shell
vertex, a B-spline line wire, and an analytic plane sheet. Directed cases include reversed/crossing/touching/coincident wires, tolerance gaps,
coincident/overlapping sheets, tiny geometry, far-origin translation, and each
option flag with non-manifold mode. There are no file-reader
inputs or analytic-to-B-spline substitutions for plane sheets. The constructor's
automatically calculated tolerances are outside this unit: the shim supplies
1e-9 distance and 0.3 radians, initializes the manifold/loop flags, and configures
all five options explicitly. Each call owns its contexts and native topology.

Coverage limitations: the shared merge_p1 temporary-transform helper currently
returns `Unported(query.brep)` for every non-null operand pair. Those cases run the
whole C++ operation but are deferred on the Rust side, including C++ success paths.
Only null-operand errors can establish parity at present. Their status is data,
so both sides compare the post-operation doing-Boolean flags, editing flags,
surviving operand graphs and intersection/component counts even on errors.
The native shim exports each full graph; the runner imports only the native
post-state on the C++ side and serializes it through the shared brep_p10 traversal
serializer (exact integer topology, geometry samples under the normal 1e-9 rule,
plus lazy gap/UV state). The Rust side serializes its independently mutated arenas.
Error-state matches remain **zero Boolean success paths**, even though they now
compare values. All eleven operations lack success-path parity. The complete
classification/copy/delete/region-bookkeeping tail has source-review evidence only.
Do not promote this unit to green from these cases.

The null-input cases exercise the same input-validation concern as
`TestKernelMergeBrepsInvalidInput`, directly at this unit's NER checks; they do not
claim coverage of the higher-level list API's singleton/duplicate-list contracts.
Material census, BooleanLists, PiecewiseMerge and real solid volume results remain
outside the tested surface until the missing dependencies are wired.

`KernelManifoldIntersector` implements the ported merge and intersector stages. It owns
`MergeState<Relations>` and one notification-record store, and borrows the caller's
AttributeStore. Face, edge and vertex mates live only in the canonical Relations;
common lists use the same helper as TopologyIntersector, while mapped lists retain
its deterministic other-handle order. The four merge_p1 stages and merge_p5 wire
processing are called directly. Paired wire processing uses each operand's mark
context and the other operand's live context for vertex-copy notifications.
Region propagation resolves source keys to the actual operand region records.

A caller reserves one 27-bit namespace per operand pair in shared attribute and
notification stores. Keys distinguish equal arena ids by operand and entity class;
source-list handles resolve to the lists in the same merge attributes. Partial
surface subsets use the canonical B-rep-cache owner identity convention, including
surfaces with a shared owner. No geometry or claimed dependency algorithm is copied.
The batch #259 wiring calls the real intersector_p2 IntersectInsertRelate,
RemoveRelationship, FixGaps and DeleteFaces methods. Typed mate maps are moved
through that same generic intersector and restored after errors; proto state and
curve handles persist in the adapter. The arena adapter borrows both operands,
caller attributes and notification records. Primary notification callbacks are
scoped separately from the live kernel's mark/tolerance state and restored on
every Result exit; distinct other-context callbacks remain live.
Transform delegates to brep_p5 with real notifications, FindAndSetInfiniteRegion
to brep_p5, and OrientSheetSurfaces to brep_p9. CopyFaces and vertex-copy calls
also use main's expanded notification interfaces. Only genuinely missing calls
retain `UNPORTED(<owner>)` tags at their actual dependency boundary.
The differential runner now instantiates this kernel adapter and reads its options
from MergeState. Focused kernel tests exercise its ported fast paths, mate ordering,
cleanup on errors, propagation/callback identity, distinct-context marks and subsets.
Those tests do not establish full Boolean success parity: **no differential case
currently compares past the null checks**, because query.brep still blocks first.

`CloseIntersectionLoop` is listed as unused and absent from all unit assignments,
although `SmMerge.cpp:2958` calls `SmTopologyIntersector.cpp:6108-6354`. Per overseer
direction it is stubbed under its proposed owner `boolean.topology_intersector_p2`;
#203 landed without it and remains partial, so this is still that owner's
missing part, pending the overseer's follow-up co-port. No implementation is
duplicated here, and the caller's retry/cleanup tail is retained.

Scope and ownership: the temporary transform now returns an inverse token whose
scope covers the complete operation and restores only surviving operands on
error and imprint exits. The wire/face-copy adapter calls borrow the caller's
same AttributeStore through the concrete adapter. Native snapshots
include every extracted component graph (record length, editing flag, graph),
in primary-then-other component order. A plane-domain constructor rejection
(width 2000000) exercises cleanup of geometry not yet owned by a face.

The #203 landing was rechecked: CloseIntersectionLoop is absent. Its proposed
owner tag must be replaced when that follow-up lands. IntersectInsertRelate now
reaches intersector_p2's existing GetFaces boundary (tracked for that owner's
follow-up in COORD); p4 no longer stubs the ported driver itself.
The FFI supports coordinates/dimensions up to 1e100 via shared reader bounds.
Its operand families avoid the closed-edge UB-175 path; adding closed curves
requires a new guard review.
