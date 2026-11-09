# topology.shell: borrowed region context for attribute backedges (PR #80, issue #86)

RegionLookup resolves an opaque attribute ObjectKey to a RegionRef containing the
actual borrowed Brep, its caller-supplied typed identity, and the scoped RegionId.
RegionRef invokes the implemented region getters. NULL owners remain None and
are distinct from valid B-rep identity zero. Void state remains the raw native
integer; boolean attribute consumers use nonzero truth, including UNSURE (2).
The attribute store retains only attributes and their object lists, not a second
copy of region flags or a global topology registry.

merge_with_regions, notify_with_regions, sag_notify_with_regions and topology's
notify_with_regions pass that borrowed context through existing handlers. Topology
mark propagation and notification callback order are unchanged. Legacy entry
points remain usable for generic attributes; a region handler without lookup
returns an explicit missing-context error rather than an owned-unit UNPORTED
marker or inferred metadata. Future callers that carry opaque region keys must
supply the actual mapping. This is an additive API proposal for review in #86.

The existing native attribute fixtures construct real detached SmRegion objects.
Their Rust counterparts now allocate actual detached Region records in typed
arena storage and map only object identities; default flags match SmRegion().
They do not derive region state from merge-region attribute classifications.
The new kernel regression also distinguishes equal scoped indices in two B-reps,
NULL versus owner identity zero, infinite-region membership and raw void state 2.
Owned/multiple-Brep identity is tested there; the existing native attribute scripts
exercise their actual detached-region constructor domain.

Validation: strict CPP workspace Clippy and 27 targeted kernel tests pass.
geom.attribute compares 2029/2029 exactly (38 former deferrals removed) and its
explicit --status-of result is green. geom.a_object compares 1909/2040 exactly
(37 former deferrals removed), leaving only topology.brep_cache's 131 deferrals.
The other six affected units retain zero failures; all compared cases across all
eight units are sensitive. Shell coverage remains 557/571, with 14 genuine callee
deferrals. Full final shell gates and remaining native coverage are still pending.
Only stale ownership comments change in geom.tol and intersect.point_class;
there is no new SSI implementation or external corpus use.
