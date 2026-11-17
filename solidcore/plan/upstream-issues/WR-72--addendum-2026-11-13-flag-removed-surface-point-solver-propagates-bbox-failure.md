Updates: WR-72

#### WR-72 (update, 2026-11-13). `surface_point_solver_propagates_bbox_failure` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A13a, which retires three surface flags). `SurfacePointSolver::new` (`surface/surface_point_solve/p1.rs`) returns the error of a failed `Surface::calculate_bounding_box` for `SolverOperation::Signed3dDirectedMaximize`, where upstream keeps the inside-out box and solves against a test point about `1.4 * SM_BIG_DOUBLE` away.

- **Tests:** `wr72_propagates_bbox_failure_in_constructors` and `wr72_propagates_bbox_failure_in_end_to_end_solves` run on `Kernel::default()`. `wr72_non_directed_ops_ignore_a_failed_bbox` and `wr72_valid_surfaces_directed_maximize` keep their checks on one kernel. `wr72_default_ignores_failed_bbox_and_places_test_point_near_big_double`, which pinned upstream's result, is gone.
- **Results:** 28 goldens rows move, all to status 1006 (`SM_ERR_NULL_POINTER`, the failed box's own status), with no output except three `p3` rows whose output stays `[0, 1]`: `surface.surface_point_solve_p1` 10, `_p2` 11, `_p3` 7. Before, 23 of them returned a result solved against the corner of the inside-out box (`p1` gen0270 returned 1.4e20, gen0364 and gen0871 points beyond 1e20) and 5 failed with 1001. No other unit moves.
