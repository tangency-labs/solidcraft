# blend.fillet_geom_p2: `--fuzz 30000` `SrfSrf` case with no C++ result in 120 s (2026-10-21)

- Open, `fuzz23694` (`SrfSrf`, base `gen0060`, `/query/s1/b_spline/net/0/1/2 = 1.0`): a valid input on which the C++ gives no result within 120 s, in `SmLSIGlobalSolver::LocalSolve` -> `SmSurface::LocalPointSolve` under a deep `SmGlobalSolver::SolveBranch` recursion (gdb after 40 s). Not guarded (a valid input); not yet known whether it terminates.
- Found by raykholo's PR #358, stacked onto main.
