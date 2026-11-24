Updates: WR-440

#### WR-440 (update, 2026-11-15). `check_loop_projected_seam_ends` is removed; `CheckLoop` compares a projected seam end on the nearest period

`check_loop` (`topology/trimming_tools.rs`) always lifts an end that has no UV trim (projected from its vertex) to the period nearest its neighbour in a closed direction. Exact UV trim ends keep their sheet, and the 3D gap is measured at the original parameters, as before. Removed: the field, SolidCraft's preset line, the rig's `fixes.json` entry, the flag-list test entry, and the tests' flag-off halves (`issue_530_projected_rotational_sweep_loops_cross_periodic_seams`, `wr_440_revolved_line_seam_check.rs`).

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes: a seam of a whole turn with projected ends is no longer an `SM_ERR` (1001) or a closure 1.
- **Goldens:** `topology.trimming_tools` `projected_seam_cone_eu0..3` (the record's cases on `cone.smb`): closure 1 or `SM_ERR` with a UV gap of 1.53 against a limit of 15.3 becomes closure 0 with no report. The jump was one period of the cone's closed U direction, and `u` and `u + period` evaluate to the same point of a closed surface, so the ends meet.
- **Tests:** `wr_440_revolved_line_seam_check.rs` checks that the three revolved lines pass `check_brep` Level 2 on the default kernel with no `SmTrimmingTools` report; `issue_476_and_530_fixes_combine_on_a_mixed_seam_gap` keeps the case where both fixes close a loop and checks that the same 3D gap is closure 3 at the vertices' own tolerances.
