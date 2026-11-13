# primitive.primitive_creation_p2: planar-path sweeps at 1e4-1e6 scale fail their union after ProjectAndTrim (2026-10-25)

Cases `gen0003`, `gen0028`, `gen0039`, `gen0178`, `gen0200`, `gen0208`, `gen0252`
(op 9, `CreateSweepAlongPlanarPath` of a circle along two line paths at lengths
1e4..1e6 with a 360-degree rotation, tolerance `d[0]` 0.1..10). They were deferred on
main at `MergeCurvesOnSurface`'s classification world; with the merge running over the
B-rep itself they reach the end and disagree: C++ returns success (`record[0]` 0,
`record[10]` 1), Rust `SM_ERR` from `boolean::merge::p3` (the union of the trimmed
pieces), with the context's `DoingBoolean` flag left set (`record[2]` Rust 1, C++ 0).

In Rust each `ProjectAndTrim` merges its cutting line into the cylinder face (one
interval, Vertex/Edge/Vertex, one new face) and then fails in
`sm_FindSideOfCurve(reference)`: the reference point is 0.1 from the cutting line and
the tolerance is the approximation tolerance (up to 10), so "point is on the curve"
gives `SM_ERR`, which the planar-path code ignores. That step follows upstream exactly
(same inputs give the same status in C++), so the divergence is in what the merge left
on the pieces or in the union itself; not traced. Two separate follow-ups: the merge's
B-rep after each `ProjectAndTrim` needs comparing with C++ for these inputs, and the
`DoingBoolean` flag the failed union leaves set (upstream's `SmTemporaryChangeValue`
restores it on every exit). The 7 cases are known mismatches until then; 3 of the 21
formerly deferred planar-path cases (`gen0092`, `gen0189`, `gen0226`) now pass and 11
moved to later deferrals.
