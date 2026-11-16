#### Updates: UB-252 (tangency-labs/solidcore#458)
- **Status in the port:** fixed, as the fork's tangency-labs/smlib@9a12ad7 does, before this addendum; nothing new was needed for #458.
  - `VariableRadiusFs::setup_offset_values` (`crates/solidcore/src/blend/fillet_standard_solver/p1.rs`) returns `FilletLaw::evaluate`'s failure (`?`) before it sets either offset distance.
  - `TemporaryRadiusChange::set_up` (`crates/solidcore/src/blend/fillet_executive/p2.rs`) evaluates the end radius (`law_evaluate(..)?`) before it swaps in the constant B-spline law or overwrites the linear law's radii, so a failed evaluation returns with the law untouched; `TemporaryRadiusChange::new` ignores that status as upstream's constructor does, and the solver then works on its own law, whose next evaluation (`setup_offset_values`) reports the failure.
  - `FilletSolver::step_solve` (`crates/solidcore/src/blend/fillet_solver/p3.rs`) returns the failed `setup_offset_values` (test `step_solve_returns_a_failed_setup_of_the_offsets`).
- **Opt-in:** none. Upstream's `SetupOffsetValues` never reports a failure and its reads of the unset radius are undefined, so no result the difftest compares changes.
- **Not covered:** the second `StepSolve` call (`SmFilletSolver.cpp:5321`) sits behind the variable-radius branch `step_solve` still defers (`GetOriginalCurve`, unit `blend.fillet_standard_solver_p1`); it must check the status when that branch is ported.

Fixes tangency-labs/solidcore#458
