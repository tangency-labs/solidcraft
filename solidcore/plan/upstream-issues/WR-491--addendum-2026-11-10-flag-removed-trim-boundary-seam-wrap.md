Updates: WR-491

#### WR-491 (update, 2026-11-10). `trim_boundary_seam_wrap` is removed

`CreateOrientedTrimBoundary` takes a UV gap of whole periods in a closed direction between consecutive curves for a seam crossing, instead of filling it with a line across the whole domain. That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. Every goldens unit (283) gives the same output at this change as at main `87400c80e`, run on Windows. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** in `tests/trim_boundary_seam_wrap.rs`, `wr491_seam_wrap_fix_gives_the_exact_frustum`, `wr491_seam_wrap_fix_keeps_a_sound_body`, `i1437_pole_gaps_are_not_seam_crossings` and `i1437_sphere_properties_with_the_fix` now check the exact values only. The default-off test, `wr491_seam_wrap_collapses_the_lateral_face_by_default`, is deleted, and so are the flag-off halves of the bit-for-bit comparisons.
