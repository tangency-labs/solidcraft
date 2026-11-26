Updates: WR-345

#### WR-345 (update, 2026-11-14). `offset_walls_touch_refused` is removed

With `offset_resolve_crossing_loops`, `ResolveCrossingLoopsOfFace` looks at every contact before any crossing and refuses loops that touch without crossing as `SM_ERR_OFFSET_WALLS_TOUCH` (1042, the fork's a759622), not as whichever crossing check fails first (`SM_ERR_BAD_INTERSECTIONS`). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::offset_walls_touch_refused`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/issue_1442_offset_walls_touch.rs`, `tests/issue_1442_local_operation_offset_status.rs` and `tests/fork_1b016ca_cavity_floor_under_opening.rs`; the tests of the path without it are removed.
