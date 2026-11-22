Updates: WR-99

#### WR-99 (update, 2026-11-21). `local_surface_intersect_guards_max_boundary_retry` is removed

The fix is the default and its flag is gone. `Surface::local_surface_intersect` (`surface/surface/p11.rs`) always guards both the minimum and the maximum boundary tests of its second through fourth out-of-bounds boundary retries (`v` of this surface, `u` and `v` of the other) with `!found_solution`, as the first retry does, so a failed solve at a maximum boundary never overwrites a boundary solution already found. Removed: the flag's file `kernel/fixes/local_surface_intersect_guards_max_boundary_retry.md`, its entry in `issue_18166`'s list, the test `wr99_flag_metadata_and_toggle` (it pinned only the flag), and the flag-off halves of `wr99_surface1_u_solution_not_lost_on_v_max_retry`, `wr99_other_surface_max_retries_do_not_clobber_earlier_boundary_hits` and `wr99_true_max_boundary_solution_still_found_when_prior_checks_miss` (each keeps every assertion on the default kernel).

- **Results:** no goldens row and no kernel-rig case moves.
- **Tests:** the three `wr99_*` tests in `surface::surface::p11` run the guarded retries on the default kernel and find the exact boundary intersections.
