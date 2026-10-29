#### WR-497. `SmShell::IsInnerShellOf` decides from statuses it ignores
- **Severity:** Wrong result
- **Area:** Topology (shells, containment)
- **Location:** `source/SMLib/src/SmShell.cpp`, `SmShell::IsInnerShellOf` (`:827-1161`): `ClosureTraversal` of the outer shell, `SmBrep::CalculateBoundingBox`, `SmFaceuse::GetPointsInFace`, `SmCurve::EvaluatePoint` of a wire edge, `SmTopologySolver::BrepCurveSolve`, `SmCurveClassification::InsertTopologyIntersections`, and the crossing face's `EvaluatePoint` / `EvaluateNormal`.
- **What happens:** every status is ignored. A failed closure traversal leaves `bIsClosed` FALSE (the shell is "not inside"); a failed box leaves the ray length from an empty extent; failed sample points leave the rays of the previous attempt or none; a ray the solver could not cast, or whose crossings could not be classified, is still parsed (an empty classification reads as "sailed out into space", an inner shell of the infinite region); a crossing face whose normal could not be evaluated orders the shells by an unset vector. Each gives a definite TRUE or FALSE that is not evidence.
- **Trigger:** any failure of those callees; none is known on valid closed bodies (the #1473 box with a sealed void classifies correctly either way).
- **How found:** port of the function for tangency-labs/solidcore#1419 (WP2) and #1473; the fork's status pass (tangency-labs/smlib 4038552).
- **Confirmed:** no (read only).
- **Fix (fork 4038552):** `UNSURE` when the closure, the box, the sample points or a crossing face's point or normal cannot be computed; a ray that cannot be solved or classified decides nothing and the next ray is tried.
- **Rust port:** opt-in `Kernel::fixes.shell_inner_shell_of_statuses` (default `false`, upstream's behaviour) in `topology::shell::is_inner_shell_of`. Test: `crates/solidcore/tests/issue_1473_inner_shell.rs` (both settings).
