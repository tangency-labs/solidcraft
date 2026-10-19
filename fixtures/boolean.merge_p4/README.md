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

`KernelManifoldIntersector` implements all sixteen required methods. It owns
`MergeState<Relations>` and one notification-record store, and borrows the caller's
AttributeStore. Face, edge and vertex mates live only in the canonical Relations;
common lists use the same helper as TopologyIntersector, while mapped lists retain
its deterministic other-handle order. The four merge_p1 stages and merge_p5 wire
processing are called directly. Paired wire processing uses each operand's mark
context and the other operand's live context for vertex-copy notifications.
Region propagation resolves source keys to the actual operand region records.

A caller reserves one 28-bit namespace per operand pair in shared attribute and
notification stores. Keys distinguish equal arena ids by operand and entity class;
source-list handles resolve to the lists in the same merge attributes. Partial
surface subsets use the canonical B-rep-cache owner identity convention, including
surfaces with a shared owner. No geometry or claimed dependency algorithm is copied.
Only genuinely missing calls retain the trait's `UNPORTED(<owner>)` defaults.
The differential runner now instantiates this kernel adapter and reads its options
from MergeState. Focused kernel tests exercise its ported fast paths, mate ordering,
cleanup on errors, propagation/callback identity, distinct-context marks and subsets.
Those tests do not establish full Boolean success parity: **no differential case
currently compares past the null checks**, because query.brep still blocks first.

`CloseIntersectionLoop` is listed as unused and absent from all unit assignments,
although `SmMerge.cpp:2958` calls `SmTopologyIntersector.cpp:6108-6354`. Per overseer
direction it is stubbed under its proposed owner `boolean.topology_intersector_p2`;
the overseer will co-port it in that unit in a follow-up. No implementation is
duplicated here, and the caller's retry/cleanup tail is retained.

Scope and ownership: the temporary transform now returns an inverse token whose
scope covers the complete operation and restores only surviving operands on
error and imprint exits. The wire/face-copy adapter calls borrow the caller's
same AttributeStore through the concrete adapter. Native snapshots
include every extracted component graph (record length, editing flag, graph),
in primary-then-other component order. A plane-domain constructor rejection
(width 2000000) exercises cleanup of geometry not yet owned by a face.

The #203 coordination explicitly requires rechecking CloseIntersectionLoop when
that unit lands/goes green; the proposed-owner tag must not outlive its co-port.
The FFI supports coordinates/dimensions up to 1e100 via shared reader bounds.
Its operand families avoid the closed-edge UB-175 path; adding closed curves
requires a new guard review.
