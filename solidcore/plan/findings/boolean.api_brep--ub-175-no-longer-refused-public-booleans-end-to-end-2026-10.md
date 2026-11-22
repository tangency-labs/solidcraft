# boolean.api_brep / boolean.topology_intersector_p2: UB-175 no longer refused; public booleans end to end (2026-10-24)

`TopologyIntersector::intersect_insert_relate` used to return `InvalidInput` wherever
upstream's `~SmProtoTopologyManager` reads a freed proto edge (UB-175). Every Boolean
with a closed intersection loop (a through-hole, a sphere cut by a plane, a torus cut
in half) has a proto edge whose ends coincide, so this refused the most ordinary
Booleans; upstream reads freed memory there and in practice goes on. The port's arena
teardown is defined, so it now goes on with its result and records the condition on
the kernel (`Kernel::upstream_ub_passed`, `"UB-175"`). The difftest keeps classifying
these cases as before: `boolean.api_brep`'s C++ guard rejects the input when the Rust
run records UB-175 (it used to look for the error), and the `boolean.topology_intersector`
runner reports -2 for `IntersectInsertRelate` when the condition is recorded, as the
shim's verbatim copy does. No case moves from guarded to compared.

Cross-check against the SMLib reference build (`bin/cpp`, the SMLib API): the seven
cases of `crates/solidcore/tests/boolean_api.rs` (box minus cylinder through-hole,
box union box, sphere intersect box, T-pipe, coplanar union, tangent sphere/box union,
torus minus box), three of which reach UB-175, give the same face counts as SMLib and
volumes within 1e-11 relative of the SMLib volumes (and within 1e-6 of the analytic
ones):

| case | Rust volume | SMLib volume | faces |
|---|---|---|---|
| box-cyl-hole | 874.336289861456 | 874.336289861 | 7 |
| box-box-union | 1875 | 1875 | 12 |
| sphere-and-box | 410.5014651122066 | 410.501465112 | 2 |
| tpipe | 308.95669741677193 | 308.956697417 | 5 |
| coplanar-union | 1500 | 1500 | 8 |
| tangent-sphere-box | 1113.0973391661814 | 1113.09733917 | 7 |
| torus-minus-box | 111.03306208872733 | 111.033062089 | 3 |

The same change wired the context gaps those Booleans reached on the public path:
`SmBrep::MakeEdgeInFaceWithVU`'s curve drop now runs the fast-drop trim test on its face
(`Surface::drop_curve_face_tested`), `SmFace::Point3DClassify` is an `IntersectorBrep`
method (`ArenaPair` and the runner's `Two` implement it) and runs on the face's B-rep in
`face_intersect_breps`, `SmBrep::Notify` resolves region attributes against the notifying
B-rep (`SmRegion::GetBrep`), the surface intersector and the line/curve intersections it
runs take the trees of the face-aware context's caches as built (that context's
`GetTree` ran through the face; a second one in the same state rebuilds the same tree),
and `sm_CoverOtherCoincidentFace` runs its `STEPInversion` solves under a face solve
context lending the B-rep (cherry-picked from fix/cover-coincident-face-ctx: 94c154d67,
f8c04179b, dabd7f67d, d88f2c9ca; the context is `query::brep::BrepFaceServices`, main's
B-rep face solve context, in place of that branch's unlanded `BrepFaceSolveContext`).
