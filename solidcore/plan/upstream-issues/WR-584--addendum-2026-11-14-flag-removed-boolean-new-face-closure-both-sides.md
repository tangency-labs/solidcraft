Updates: WR-584

#### WR-584 (update, 2026-11-14). `boolean_new_face_closure_both_sides` is removed

`SmBrep::TestClosureSplitRegion` always asks `ClosureTraversalFromBothSides` (the fork's tangency-labs/smlib@69c5198) whether a new face closes a region; upstream's one-sided `ClosureTraversal` walk is no longer called there. The flag is gone: the field, SolidCraft's preset line, its entry in `fixes.json`, the goldens' name map and every test setting of it. `closure_traversal` itself stays (healing and `SmShell` call it).

- **Results:** the answer is the same, so no status, shell, region, face, edge or geometry output moves. The 75 moved rows of `topology.brep_p6` change only in the mark state the runner records: each closure test claims one mark fewer (the mark type's `current_mark` advance drops by 2 per call), and the open-box cases no longer leave the walk's marks on 9 face uses.
- **Tests:** `crates/solidcore/tests/fork_69c5198_closure_both_sides.rs` (the two-sided search marks nothing; both traversals agree on the cube and the open box; a union of two boxes overlapping at a corner is the 12-face, 30-edge, 20-vertex polyhedron).
