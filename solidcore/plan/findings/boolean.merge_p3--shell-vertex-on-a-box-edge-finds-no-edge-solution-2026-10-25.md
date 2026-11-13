# boolean.merge_p3: shell vertex on a box edge finds no edge solution (2026-10-25)

Cases `piecewise_box_other_shell_vertex_imprint_2` and `_3` (fixture
`fixtures/boolean.merge_p3/cases.json`, family 4: `PiecewiseMerge` of the box
`(0,0,0)+(10,10,10)` with a one-vertex B-rep at `(5,0,0)`, options 2 and 3). They were
deferred on main at the first face solve of `sm_IntersectShellVertices`
(`smsurf_GetSurfaceCache` of a face-owned surface, "SmFace handle of a surface's
face"). Now that the intersector's topology solves reach their faces' caches, they run
to the end and disagree: C++ ends with 13 edges (`ints.topology_counts[1]`), Rust with
12; `values.record[6]` C++ 13 vs Rust 12, `record[8]` 0 vs 5, `record[9]` 10 vs 0,
`record[13]`/`[14]` 10 vs 0, `record[19..21]` 0 vs 10, `record[23]` 5 vs 0.

Rust's `BrepPointSolve` of the vertex point against the box returns two face solutions
(the faces `y = 0` and `z = 0`) and no edge solution: the point/edge pass over the
B-rep cache's curve tree (`TopologySolver::brep_point_solve`, `trees.curves`) never
reaches a `SmTLSPointEdge` local solve (the tree's root box is
`(-1.501326..11.501326)^3`, the query box the point itself), so
`sm_FindFirstLowestDimensionSol` picks a face and the vertex is imprinted on a face
instead of splitting the edge `(0,0,0)-(10,0,0)` as C++ does. Suspected cause: the
branch culling of the point query against the curve tree (`BranchMayContainAnswers` /
`AreReadyForLocalSolve` with a zero-size point box and the vertex's zone tolerance), not
the face-cache wiring, which only made the cases reach this point. The cases are known
mismatches until that culling is compared with the C++.
