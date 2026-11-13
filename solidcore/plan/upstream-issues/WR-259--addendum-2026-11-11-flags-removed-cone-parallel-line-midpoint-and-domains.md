Updates: WR-259

#### WR-259 (update, 2026-11-11). `cone_parallel_line_midpoint` and `cone_parallel_line_domains` are removed

`Cone::intersect_with_cone`, in its branch for two parallel cylinders that do not coincide, clips each crossing line to both requested face domains (converted to STEP) and tests it against both faces at its middle. This is now the only path, and both flags are gone. Upstream builds each line over the first cylinder's full axial extent and tests its bottom end, so a face trimmed above that end loses the line, and a face shorter than the cylinder gets a line that runs past it. SolidCraft and the kernel rig already ran with both flags on. Removed: both fields, SolidCraft's preset lines, their entries in `fixes.json`, the goldens case flag arm, every test setting of them, and the flag in the four cases of `fixtures/solidcraft/3665-column-in-channel.json`.

- **Results:** the default kernel now gives the result SolidCraft already had. The goldens rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.cone_p3--parallel-cylinder-lines-clipped-by-default-2026-11-11.md`.
- **Tests:** `surface/cone/p3.rs`: the `issue711_*` tests run on the default kernel; `issue711_domain_clipping_is_independent_of_midpoint`, which compared the flags off and on, becomes `issue711_lines_end_where_either_face_ends`, the fixed half. `tests/issue_711_overlapping_holes.rs`: both holes cut on the default kernel; `issue_711_second_overlapping_hole_fails_by_default`, the flag-off half, is removed.
