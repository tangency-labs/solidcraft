Updates: WR-497

#### WR-497 (update, 2026-11-10). `shell_inner_shell_of_statuses` is removed

`SmShell::IsInnerShellOf` passes on the statuses upstream ignores (fork 4038552): `UNSURE` when the outer shell's closure, the B-rep's box, the sample points or a crossing face's point or normal cannot be computed, and the next ray when a ray cannot be solved or classified. That is now the only path, and the flag is gone, with the `Checked::Ignored` arms it kept. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. Every goldens unit (283) gives the same output at this change as at main `87400c80e`, run on Windows. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** `issue_1473_box_with_sealed_void` and `issue_1473_failed_closure_traversal_is_unsure`. The upstream-status tests, `issue_1473_box_with_sealed_void_upstream_statuses` and `issue_1473_failed_closure_traversal_upstream_is_false`, are deleted.
