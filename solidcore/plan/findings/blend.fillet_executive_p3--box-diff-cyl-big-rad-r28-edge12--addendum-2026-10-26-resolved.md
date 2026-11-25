# blend.fillet_executive_p3: box_diff_cyl_big_rad edge 12 at radius 28, resolved (2026-10-26)

Updates: plan/findings/blend.fillet_executive_p3--box-diff-cyl-big-rad-r28-edge12.md

First diverging point: none of the prefix checkpoints. Every checkpoint from `prepared`
to `merged_fillet_faces` agrees bit for bit on this input, including the UV-trim and
surface-domain samples. The two kernels separate after `DoFilleting` returns.

What both kernels do in the post-merge tail: the target has 3 faces and 8 edges. All five
stitch attempts leave 3 lamina edges, `RemoveTopoEdges` and `BooleanPostProcess` change
nothing, and `OrientTrimmedSurfaces` returns `SM_ERR_ASSERT_FAILURE` ("Nonorientable
Brep"), which upstream ignores. The cylinder face's loop is `[(e2, 2), (e0, 2), (e1, 1),
(e0, 1)]` (edge, orientation code) on both sides, so edge `e2` has the same orientation in both of its faces.

Root cause: the Rust-only closed-manifold check at the end of `do_fillet` (WR-212,
tangency-labs/solidcore#422) refused this target with status 1001 (`SM_FILERR_BAD_MERGE`).
The C++ `DoFillet` returns `SM_SUCCESS`, and `SmApiFilletEdges` then runs `StitchAndOrient`.
That reverses the cylinder face (its parameters swap, which explains the
`result_geometry[171..181]` samples and `result_topology[88]`/`[92]` orientations in the
original record), and the result is manifold. The Rust port returned before
`StitchAndOrient`.

Fix: the check is now opt-in (`Kernel::fixes.fillet_refuse_non_manifold_result`, off by
default; see `plan/upstream-issues/WR-212--addendum-2026-10-26-opt-in.md`). The case is a
fixture in `fixtures/blend.fillet_executive_p3/big_radius_r28.json` and passes (217 values
compared).
