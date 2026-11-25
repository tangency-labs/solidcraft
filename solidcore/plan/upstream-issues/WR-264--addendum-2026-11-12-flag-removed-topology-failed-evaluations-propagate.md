Updates: WR-264

#### WR-264 (update, 2026-11-12). `topology_failed_evaluations_propagate` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15d). `calc_dist_and_dev` (`topology/edgeuse/p1.rs`) gives no distance (`-1`) without the surface's derivatives or normal.

- **Tests:** `issue_521_dist_and_dev_is_none_without_surface_derivatives` runs on `Kernel::default()`; `failed_evaluations_are_ignored_by_default` is gone.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
