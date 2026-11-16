Updates: WR-19

#### WR-19 (update, 2026-11-10). `local_solve1d_unbounded_newton_step` is removed

Without an interval, `LocalSolve1d::solve_it` (`SmLocalSolve1d::SolveIt`) takes unclamped Newton steps. Upstream clamps each step against the default inside-out `[1e20, -1e20]` interval and so replaces every step with `±1.584e19` against the Newton direction. The Newton step is now the only path, and the flag is gone. Removed: the field, `LocalSolve1d::new_with_kernel`, `with_interval_and_kernel`, `set_unbounded_newton_step` and `unbounded_newton_step`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `geom.local_solve1d` changes in 139 cases (170 solves), checked by evaluating each test function at the returned parameter:
  - solidcore returns a finite parameter in 170 of 170 solves; upstream's behaviour does in 27.
  - Roots found within the acceptable accuracy: 104 against 12.
  - In 17 solves, upstream's behaviour reports "found" near 1e19 with residuals up to 1e19.
  - In 7 Gaussian-bump cases, upstream's wrong-way step happened to create a bracket that bisection then solved. Pure Newton walks into the flat tail there instead.
- **Tests:** the WR-19 unit tests in `geom::local_solve1d`.
