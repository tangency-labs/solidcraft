Updates: WR-262

#### WR-262 (update, 2026-11-12). `topology_failed_evaluations_propagate` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15d). `uv_corner_boundaries` (`topology/loopuse.rs`) takes a trim curve that cannot be made as none and adds no boundary for an edge use whose ends cannot be evaluated (the same flag covered WR-263 and WR-264).

- **Tests:** `issue_521_uv_corners_skip_an_edgeuse_whose_ends_fail` runs on `Kernel::default()`; `uv_corners_ignore_failed_trim_evaluation_by_default` is gone.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
