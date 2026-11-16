#### DOC-4. Revolution STEP equal-order assertion conflicts with boundary iso evaluation
- **Severity:** Docs
- **Area:**
- **Location:** `source/SMLib/src/SmSurfOfRevolution.cpp:2770-2793`; caller `SmIsoCurve::Evaluate` in `SmIsoCurve.cpp:123-160`.
- **What happens:** The STEP evaluator documents and asserts equal U/V derivative orders, but iso curves request `(k, 0)` or `(0, k)`. Release C++ computes sweep-only `(k, 0)` derivatives correctly; rejecting every unequal order prevents legitimate boundary point solves. When V exceeds U, generator derivatives beyond `min(U, V)` retain the default `SmVector3d` `SM_UNDEF_DOUBLE` sentinel.
- **Trigger:** `LocalPointSolve_1` on a STEP-wrapped revolution at its minimum-V boundary, regression `regression_matrix_0363` in `fixtures/surface.surface_point_solve_p3/step_revolution_regressions.json`.
- **How found:** native differential regression while porting `surface.surface_point_solve_p3` (PR #106).
- **Confirmed:** yes for the release build: the sweep-only boundary solve returns U `158.85355268828013`; the assertion conflict and sentinel initialization were confirmed by source inspection. Debug builds were not run.
- **Severity:** docs / debug assertion.
- **Rust port:** Matches release C++ rather than enforcing the assertion; derivative orders remain bounded to 3.
