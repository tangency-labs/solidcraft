# blend.fillet_geom_p1: `sm_ClassifyFacePoint` returns 0 where the C++ returns 1 or 2 (2026-10-22)

Found while batching PR #268 (`blend.fillet_corner_p2`) onto main. Now that
`topology.face_p4` (`GetUpwardLoopuses`) and `blend.fillet_geom_p6` (`ShouldSwapFaces`)
are ported, #268's `update_split_face` corner cases run `sm_ClassifyFacePoint` instead
of deferring, and 5 of them fail (`gen0014`, `corner_update_split_face_08/_11/_71/_75`).
In each, one fillet vertex's point-classification face stays at the original face
(Rust, `which = 0`) where the C++ moves it to the new face (`which = 2`).

The same disagreement shows in `blend.fillet_geom_p1`'s own `classify_face_point` op
(main's code, not #268's), with the two planar bilinear faces of that corner model
(face 0: the `y = 0` square `[0, 0.1] x [0, 0.1]` in x/z; face 1: the `x = 0` square in
y/z). The weights are 1.0 or non-rational; both give the same results:

| pos                  | C++ | Rust |
|----------------------|-----|------|
| `(0, 0.0179, 0)`     | 2   | 0    |
| `(0, 0.05, 0.05)`    | 2   | 0    |
| `(0.05, 0, 0.05)`    | 1   | 0    |
| `(0, 0.0179, 0.001)` | 2   | 2    |

Suspected cause: the drop or `Point3DClassify` step of `classify_face_point`
(`crates/solidcore/src/blend/fillet_geom/p1.rs`) reports neither face for points well
inside one face. Its existing fixture cases (a 3 x 2 face pair) pass. This has not been
investigated further. Until it is fixed, #268's runner cannot be green. Also, #268's
`p3_check_split` shim guard (`-3` for `ShouldSwapFaces`) is stale on main, because
`blend.fillet_geom_p6` ported it.
