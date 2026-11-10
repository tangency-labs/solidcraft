Updates: WR-602

#### WR-602 (update, 2026-11-10). `fillet_cleanup_removes_input_slits` is removed

With the fork's keep (`fillet_cleanup_keeps_target_faces`), `BooleanPostProcess` checks an edge in `CleanupRelations::kept_edges` that has one face on both sides like any other edge, and the test for an edge between kept faces needs at least two faces (the fork's tangency-labs/smlib@cac11ea). This is now the only path, and the flag is gone. SolidCraft already ran with the flag on. Removed: the field, SolidCraft's preset line, its goldens mapping, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey of #14246 found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/fork_cac11ea_fillet_cleanup_removes_input_slits.rs`: `wr602_input_slit_removed_seam_kept` (the slit goes, the seam stays, the volume is unchanged); `wr602_default_keeps_the_input_slit`, the flag-off half, is removed.
