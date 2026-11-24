Updates: WR-546

#### WR-546 (update, 2026-11-24). `fillet_patch_keeps_fillet_edges` is removed

`CreateFilletBrep` always stitches the fillet body keeping a fillet's own exact section edge where an edge of corner-patch faces only meets an edge of other fillet faces only. The default choice between the two near-equal distances, which could keep the patch's approximate isoparametric edge, is gone with the flag. Removed: the field `UpstreamFixes::fillet_patch_keeps_fillet_edges`, the preset line, its `fixes` entries in tests and fixtures, and the default-stitch branch.

- **Goldens:** the four flags are retired together (#18913), so their moved rows are proved together: 177 rows in six units (`blend.fillet_executive_p3` 138, `topology.brep_cutting` 19, `primitive.api_trim_ops` 9, `blend.fillet_solver_p2` 6, `topology.api_projection` 3, `topology.api_files` 2). Each is checked by `crates/goldens/tests/issue_18913_moved_rows_are_proven.rs`, which replays it and certifies the result: a Level 2 check of the body, every analytic face and every line or conic edge on its closed-form support, and the mass properties against a certified enclosure or a closed form. All other units pass unchanged.
- **Defaults:** SolidCraft's preset already had the flag on, so the shipped result does not change. The comparison SMLib keeps upstream's path.
- **Tests:** `wr_546_setback_patch_meets_the_rounds_on_their_own_edges` runs on the default kernel; flag-off halves elsewhere that pinned the approximate edge are removed.
