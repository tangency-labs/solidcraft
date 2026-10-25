# topology.shell WIP: child transfers and literal sibling links (PR #80)

A native probe of self-transfer on a real plane-face B-rep returns SM_SUCCESS,
clears the shell head/count/type to NULL/0/unknown, and leaves each face use's
owner and next/previous cycle intact. Faceuse now retains literal sibling links
independently of the owner's head; a canonical construction vector remains the
fallback before literal links are supplied. TransferChildrenTo preserves this
observed behavior rather than treating self-transfer as a no-op or excluding it.

The transfer adapter snapshots every affected shell head/count/type, face-use
owner/sibling link, edge-use/vertex-use owner and region list, then exercises
GetWireEdges on the post-transfer topology. A scope guard restores the native
links before B-rep destruction; this does not alter the observed result. Native
probes confirm partial/duplicate wire lists, nullable entries ignored by early
vertex branches, rejection before unsafe NULL dereferences, and real plane-face
domains at 1e-7/1/1e6. Graph export alone accepts plane domains through the native-
verified 1e6 bound; the existing edge-use-operation bound stays at 1e4. Comparison
tolerances are unchanged.

The first 234-case run exposed four fixture precondition errors: Endcap.smb has
one face-use shell with two members, whereas Mouse and box_with_large_fillet have
two nonempty face-use shells (4/4 and 7/7 members). The Endcap setup now uses native
base Remove/PostInsert operations to form two nonempty shells, independently of
TransferChildrenTo. All four cases and the nonempty-recipient coverage assertion
are retained. The corrected run passes all 234 shell cases, with zero failures,
zero deferrals and exact integer/topology agreement; perturbation detects all 234.
Edge-use p1 retains 2449 passes/273 deferrals out of 2722, zero failures, and all
2449 comparisons are detected under perturbation. Strict C++ Clippy, formatting
and the three kernel regressions pass.

Subsequent source audit found that SM_CAST_PTR explicitly returns NULL for a
head-class mismatch. GetVertex, GetWireEdge and GetFirstFaceuse now preserve that
defined NULL result rather than returning an incompatible-class error. All 52
dedicated comparisons pass for empty, vertex, wire and face-use heads, including
signed 64-bit extrema and values beyond exact double precision. The setter and
checked getters use an integer C ABI; the raw physical head and count are compared
too, so an unrelated type code cannot mask a lost head. The complete rerun passes
all 286 cases, with exact integer/topology agreement, zero failures/deferrals and
all 286 perturbations detected. Edge-use p1 remains 2449/2722 with 273 genuine
deferrals, zero failures and all 2449 comparisons detected.
The unit is still todo/draft: remaining owned operations, backedges, final gates
and the region-context coordination in #86 are not claimed complete.
