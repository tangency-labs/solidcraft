Updates: WR-573

#### WR-573 (update, 2026-11-12). `approximate_surface_bezier_starter` is removed

`Surface::approximate_surface` raises the starter's inner knots to full multiplicity (exact knot insertion) before each round of cubic Bezier patches, and a patch that runs past the control net is `SM_ERR`. This is now the only path, and the flag is gone, together with WR-726's `approximate_surface_checks_every_row` (the two work together: with this fix alone a pass is accepted when only its last row is within the tolerance). SolidCraft and the kernel rig already ran with it on. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** the goldens rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.surface_approximate--b47-five-flags-retired-2026-11-12.md`.
- **Tests:** `surface::surface::approximate::tests::wr573_default_gives_success_without_surface` and `tests/wr573_offset_wave_export.rs`'s `without_the_flag_the_writer_succeeds_with_nothing_wr573`, which pinned the defect, are removed. `wr573_bezier_starter_approximates_the_offset_wave` and `the_flag_approximates_the_offset_face` pinned the pass that only the last-row check accepts (1.62e-3 for 1e-3; 33 x 4 at 1.12e-5 for 5e-6, the C++ with eae96cb); they are removed, and the WR-726 tests on the default kernel check that a surface comes back, within the tolerance.
