Updates: WR-706

#### WR-706 (update, 2026-11-14). `fillet_section_end_trim_at_creases` is removed

With `fillet_overflow_trim_at_creases`, a round with WR-674's section ends is trimmed at its creases (each end cut by its section's plane and closed by the section's flat face) instead of subtracted, so it builds with other rounds. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_section_end_trim_at_creases`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `tests/issue_2223_section_end_trim_at_creases.rs` and `tests/issue_2314_tab_corner_arcs.rs`; the test of the path without it is removed.
