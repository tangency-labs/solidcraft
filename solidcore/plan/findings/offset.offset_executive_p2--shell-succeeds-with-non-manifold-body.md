# offset.offset_executive_p2: the shell reports success with a non-manifold, zero-volume body (2026-10-28)

- **Unit:** `offset.offset_executive_p2` (`SmOffsetExecutive::ShellBrep`, `SmApiShellBrepFull`).
- **Inputs:** the #398 part built without `fillet_chamfer_cone_on_arc`. It is a 50 x 50 x 30 box with r10 vertical rounds, a 5 mm chamfer round the base (traced, WR-421) and an r4 fillet where the chamfer meets the wall. It is shelled by -2 with extended convex edges and merged results, open at the top or closed (`issue_398_reference_without_fix` in `crates/solidcore/tests/issue_398_shell_box_rounds_chamfer_fillet.rs`).
- **Both values:** C++ v1.0.1 (a one-off native build of the same recipe) and the port agree. The status is success, and the result has 72 faces, 307 edges and 236 vertices, is not a manifold solid, and has volume 0. With WR-421's fix the same shell is a manifold solid of the closed-form volume.
- **Finding:** the shell does not check its own result. Whatever goes wrong while offsetting and merging the traced chamfer and the fillet on it, it is returned as a success. A caller only sees it by testing the body.
- **Suspected cause:** not investigated. Candidates are the offset of the traced B-spline chamfer and fillet faces, or the merge of the offset faces (`PiecewiseMerge`). The fork's earlier lead on #398 (`SM_ERR_NULL_POINTER` from `SmEdgeDefinition::Imprint` reached from `PiecewiseMerge`) was on a different build.
- **Follow-up:** find the first step that breaks the body, and decide whether the shell should fail there, which would be an opt-in fix since it changes a result. The input is reproducible with the test's recipe and the fix off.
- **PR:** #1404.
