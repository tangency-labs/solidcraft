Updates: WR-304

#### WR-304 (update, 2026-11-14). `boolean_cleanup_removes_slits` is removed

After each edge removal, `boolean_post_process` (`boolean/merge/p1.rs`) also checks the merged face's edges that have it on both sides (slits) by the same `IsTopological` test, skipping any a later removal deleted. That is the only path now, and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::boolean_cleanup_removes_slits`, SolidCraft's preset line, its entry in `fixes.json`, its name in the SolidCraft profile list of `issue_2260_gn_pipe_refined_range_end.rs` and every test setting of it.

- **Which path went:** the upstream one, which checks only the intersection edges (and every edge with `bCheckAllEdges`), so an operand edge between two faces that a merge makes one stays as a slit.
- **Results:** no golden row moves (measured with only this flag's read site turned on, and in the combined run of the batch).
- **Tests:** `issue_1120_boolean_slits.rs` now pins the merged wall's one-face edges at exactly its one seam (it compared the two paths before); `issue_575_split_rim_join.rs` loses its flag-off half and the case with Body1 built without the clean-up. `fork_cac11ea_fillet_cleanup_removes_input_slits.rs` and `issue_423_fillet_cleanup_keeps_faces.rs` need a body with a slit, which the union no longer leaves: they now build it as SMLib's union does (the union without its clean-up, then a clean-up that keeps the disc's wall lines).
