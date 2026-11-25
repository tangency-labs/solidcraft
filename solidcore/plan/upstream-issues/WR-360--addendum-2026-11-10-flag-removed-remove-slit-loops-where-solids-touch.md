Updates: WR-360

#### WR-360 (update, 2026-11-10). `remove_slit_loops_where_solids_touch` is removed

A union, intersection or difference removes the zero-area inner loops a contact curve inside a face leaves and splits the shells joined only through them (`topology::brep::p3::remove_slit_loops_where_solids_touch`, called from `boolean/merge/p4.rs` before `finish_boolean`; the fork's `SmBrep::RemoveSlitLoopsWhereSolidsTouch`). This is now the only path, and the flag is gone. SolidCraft already ran with the flag on. Removed: the field, SolidCraft's preset line, its entry in `fixes.json`, every test setting of it and its mentions in `docs/booleans.md`.

- **Results:** none change. Every goldens unit (283) passes unchanged, and the kernel rig passes, with this flag and `boolean_temporary_transform_statuses` removed together; the survey of #14246 found no golden or rig case that reaches a different result with the flag on.
- **Tests:** `tests/issue_746_slit_loops.rs`: `issue_746_slit_loop_removed_and_solids_split` (no face keeps an inner loop, two solids); `issue_746_union_touching_along_a_line_keeps_a_slit_loop_by_default`, the flag-off half, is removed.
