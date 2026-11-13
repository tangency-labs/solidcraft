Updates: WR-726

#### WR-726 (update, 2026-11-12). `approximate_surface_checks_every_row` is removed

`Surface::approximate_surface` accepts a pass of patches only when every row (and column) of it is within the tolerance, subdivides every failed row, and fails with `SM_ERR` when the last allowed pass is still outside it. This is now the only path, and the flag is gone, together with WR-573's `approximate_surface_bezier_starter`. SolidCraft already ran with it on; the kernel rig's `fixes.json` did not list it. Removed: the field, SolidCraft's preset line and every test setting of it.

- **Results:** the goldens case `surface.surface_approximate/gen0381`, listed known failing for this record (success with an achieved tolerance of 0.546 for a requested 1.38e-3), leaves `known_failing.json`. It and the other rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.surface_approximate--b47-five-flags-retired-2026-11-12.md`.
- **Tests:** `wr726_every_row_checked_keeps_the_fit_within_the_tolerance`, `wr726_every_row_checked_refuses_when_the_last_subdivision_misses` and `with_every_row_checked_the_fit_is_within_the_tolerance_2507` run on the default kernel, with the values this record gives (13 x 4 at 9.18e-4; 34 x 4 at 4.03e-6).
