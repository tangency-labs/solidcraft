# topology.face_p1 fork trap merged with main: 2 MoveSeam-with-split mismatches (2026-10-23)

Carrying the UB-137 fork trap (PR #467) onto main (face Notify #637, cone
`TrimWithDomain` #670) makes two formerly guarded cases comparable, and they disagree.
Both are `MoveSeam` with split on face 5, face properties closed in U and V with crossed
and missing seams BOTH (`props {closed_u 1, closed_v 1, crossed 3, missing 3,
near_miss 0}`, `center false`, `split true`, B-rep rebuilt):

| case | field | C++ | Rust |
|------|-------|-----|------|
| `cone.smb_1_f5_op9` | `out[5]` new seam edges, `out[6]` seam vertices | 2, 1 | 0, 0 |
| `torus.smb_1_f5_op9` | `out[4]` new faces, `out[5]` edges, `out[6]` vertices | 1, 3, 1 | 0, 0, 0 |

Status and the U/V results agree (U `SM_FSP_NO_CHANGES`, V `SM_FSP_MOVE_OUT_OF_FACE`
for the cone).

**Cause (found by instrumenting the port).** After moving the seam, `MoveSeam` re-runs
`HasSeamProblem` before splitting (`SmFace.cpp` `if(bOkToSplit && ...IsFullPeriod())`).
The face_p1 runner calls `topology::face::p1::move_seam` with its null classification
handle `H` (names loops only) and `NoGeometry`, so that call fails in
`BrepSeamObjects::new` with `InvalidInput` ("classification handles cannot name the
face's topology, curves or seam curves"). `move_seam` treats a failed check as upstream's
`FALSE` (the status is ignored), so it never splits. Upstream finds a remaining seam
problem and `SplitAtSeam`'s merges add the edges, vertices and (torus) a face.

**Not a library defect as far as seen:** a handle type that names topology
(`HealLoopHandle`) would run the check. The fix is a runner change (classifications
and `MoveSeam`/`SplitAtSeam` over handles that name the B-rep's own entities), after
which the split's `MergeInterval` imprinting (`intersect.curve_class_p6`, still
`Unported`) would likely defer these cases instead. Until then both are listed in
`crates/difftest/known_mismatches.toml`.
