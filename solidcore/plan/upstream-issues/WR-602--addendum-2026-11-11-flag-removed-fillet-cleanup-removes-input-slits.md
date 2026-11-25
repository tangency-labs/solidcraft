Updates: WR-602

#### WR-602 (update, 2026-11-11). `fillet_cleanup_removes_input_slits` is removed

With the fork's keep (`Kernel::fixes.fillet_cleanup_keeps_target_faces`, WR-459), `boolean_post_process` (`boolean/merge/p1.rs`) now always checks a kept edge that has one face on both sides, and the test for an edge between kept faces always needs two faces. That is the only path now, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json`, its `enable_fix` arm in the goldens' SolidCraft runner and every test setting of it.

- **Which path went:** the removed path was the fork's keep without the slit check. Pinned upstream has no keep at all (`CleanupRelations::kept_edges` and `kept_faces` are empty unless `fillet_cleanup_keeps_target_faces` is on), so no upstream behaviour is lost, and on the default kernel, where that keep is off, nothing reaches the changed test.
- **Results:** see the retiring PR for the goldens run on a main export. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** `fork_cac11ea_fillet_cleanup_removes_input_slits.rs` keeps `wr602_input_slit_removed_seam_kept`. The flag-off half, `wr602_default_keeps_the_input_slit`, is deleted.
- **WR-459's test changes with it:** `issue_423_fillet_cleanup_keeps_faces.rs` showed the keep through the edge inside the united disc's merged wall. That edge is a slit (one face on both sides), so with WR-602 the keep checks it and it goes, as it does without the keep: measured on the retiring PR's export, 1 one-face edge with the keep and 1 without, where the test asserted more with the keep. The test is renamed `issue_423_keep_still_removes_the_targets_own_slit` and asserts the counts agree; this body no longer shows the keep of a two-face edge.
