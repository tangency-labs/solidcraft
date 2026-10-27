Updates: WR-456

#### WR-456 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1: `SmFilletSolver::ComputePointValues` is `SmFilletSolver.cpp:3544`-`:3768` (as at c9c5979) with `double dBigStep = 1.0e-3;` at `:3621` and the retry still passing `dStep`. `bigger_step_retry_uses_the_bigger_step_with_the_fix` still passes on `main` with the v1.0.1 port; difftest agrees with the C++ 1.0.1 by default (`blend.fillet_solver_p4`: 651 cases, all pass, max_rel_err 0).
