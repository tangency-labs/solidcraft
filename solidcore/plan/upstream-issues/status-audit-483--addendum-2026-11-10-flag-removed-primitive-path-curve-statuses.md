Updates: #483 (`status-audit-483--remaining-helper-statuses.md`)

#### #483 (update, 2026-11-10). `primitive_path_curve_statuses` is removed

Pivot selection (`find_pivot_point`, upstream `sm_FindPivotPoint`), the taper's nearest-corner pivot and path extension (`extend_path_curves`, upstream `ExtendPathCurves`) return a failed curve evaluation or G1 extension before using its output. Upstream ignores the status and goes on with an undefined point or the unextended curve. Propagation is now the only path, and the flag is gone. Removed: the field, `find_pivot_point_with_kernel`, `extend_path_curves_with_kernel`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `primitive.primitive_creation_p1` changes in 4 cases, `overflow_regression_427`, `_670`, `_689` and `_919`, each with a ±1e300 coordinate. The requested extension fails, and the operation now returns that failure (`BSplineCurve::create_extended_curve_by_distance: a NURBS routine it calls failed`). Upstream returned success with that segment, or every segment, unextended: in 670, a closed path whose middle segment starts at z = 1e300, it gave an inconsistent mix of extended and unextended joints.
- **Tests:** `primitive::primitive_creation::p1` (`audit_483_pivot_and_extensions_stop_at_failed_evaluations`, `audit_483_failed_path_extension_is_returned`).
