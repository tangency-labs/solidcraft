Updates: WR-263

#### WR-263 (update, 2026-11-12). `topology_failed_evaluations_propagate` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15d). `start_point_uv_loop_gap` (`topology/edgeuse/p1.rs`) returns `SM_BIG_DOUBLE` for a gap it cannot measure (a failed curve evaluation or point drop).

- **Tests:** `issue_521_loop_gap_is_big_without_trim_and_edge_evaluations` and `wr263_public_uv_domain_failed_trim_evaluation.rs` keep only the fixed expectations; the default's measured gap is named in the test's comment.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
