Updates: WR-705

#### WR-705 (update, 2026-11-14). `offset_updates_result_tolerances` is removed

The API offset and shell run `ValidateAndUpdateTolerances(FALSE)` on their result at level 0, so each vertex and edge covers the gaps it bounds. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::offset_updates_result_tolerances`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/issue_2111_offset_patch_sheet_gaps.rs`.
