Updates: WR-72

#### WR-72 (addendum 2026-11-01) — Opt-in bounding-box failure propagation in directed-maximise `SmSurfacePointSolver`

- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Status in the port:** `Kernel::fixes.surface_point_solver_propagates_bbox_failure`, off by default. Default `SurfacePointSolver::new` (`!k.fixes.surface_point_solver_propagates_bbox_failure`) preserves pinned v1.0.1 C++ parity so `surface.surface_point_solve_p1`, `surface.surface_point_solve_p2`, `surface.surface_point_solve_p3`, and `surface.surface_p4` difftests remain unchanged.
- **Location:** `source/SMLib/src/SmSurfacePointSolve.cpp:406-470` (`SmSurfacePointSolver::SmSurfacePointSolver`), specifically line `:463` (`m_pSrf->CalculateBoundingBox(m_sDomain, &sBBox, NULL, NULL)`), at v1.0.1 (`3fd5b95`); in Rust, `crates/solidcore/src/surface/surface_point_solve/p1.rs` (`SurfacePointSolver::new`, `SurfPtGlobalSolver::new`, `SurfPtSolveStepper::new`) and its callers `Surface::global_point_solve_1`, `Surface::global_point_solve`, `Surface::local_point_solve_1`, and `Surface::local_point_solve`.
- **What changes:**
  1. Upstream `SmSurfacePointSolver` (`SmSurfacePointSolve.cpp:458-469`) converts `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE` into minimising distance along `m_sDirVec = (-crTestPoint).Unitize()` towards a corner of the 20%-expanded surface bounding box `sBBox`. Because line `:463` ignores the `SmStatus` returned by `m_pSrf->CalculateBoundingBox`, any surface whose bounding box fails (for example a `CurveBoundedSurface` without boundary curves whose cache build fails in `SmSurface::GetKnots` -> `SM_ERR_NULL_POINTER`, or an uninitialised `BSplineSurface` without a NURB -> `SM_ERR`) leaves `sBBox` in its inside-out `SmExtent3d::Init` state (`min = +SM_BIG_DOUBLE`, `max = -SM_BIG_DOUBLE`). `ExpandRelative(0.2)` then widens the inverted extent to `min = +1.4 * SM_BIG_DOUBLE` and `max = -1.4 * SM_BIG_DOUBLE`, placing `m_sTestPt` at `±1.4e20` on the wrong side of the surface and corrupting both rough-grid objective comparisons (`>= SM_BIG_DOUBLE`) and local/global directed maximisation.
  2. When `Kernel::fixes.surface_point_solver_propagates_bbox_failure` is `true`, `SurfacePointSolver::new` propagates any `Err` returned by `Surface::calculate_bounding_box` when `solver_op == SolverOperation::Signed3dDirectedMaximize`. Because `SurfPtGlobalSolver::new`, `SurfPtSolveStepper::new`, `Surface::global_point_solve_1`, `Surface::global_point_solve`, `Surface::local_point_solve_1`, and `Surface::local_point_solve` all construct `SurfacePointSolver::new(k, ...)`, the failure propagates cleanly through every solver entry point.
  3. Non-directed solver operations (`Minimize`, `Normalize`, `Intersect`, `Maximize`), valid surfaces whose bounding box succeeds, and unconditional `SolidError::Unported` propagation are unchanged under both modes.
- **Regression evidence:** Unit tests in `crates/solidcore/src/surface/surface_point_solve/p1.rs`:
  - `wr72_default_ignores_failed_bbox_and_places_test_point_near_big_double`
  - `wr72_opt_in_propagates_bbox_failure_in_constructors_and_flag_lookup`
  - `wr72_opt_in_propagates_bbox_failure_in_end_to_end_solves`
  - `wr72_non_directed_ops_unchanged_when_bbox_fails`
  - `wr72_valid_surfaces_directed_maximize_unchanged`
- **Tracking:** tangency-labs/solidcore#2169.
