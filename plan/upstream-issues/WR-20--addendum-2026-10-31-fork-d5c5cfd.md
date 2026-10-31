Updates: WR-20 (and WR-253's N-D addendum of 2026-10-31)

- **Fork fix:** tangency-labs/smlib@d5c5cfd ("an N-D solve whose guess can't be evaluated finds
  no solution"). When the first `Evaluate` in `SmLocalSolveNd::SolveIt` fails, the fork sets
  `rbFoundSolution = FALSE` (still `SM_TR_UNABLE_TO_CONVERGE`, `SM_SUCCESS`), as when an evaluation
  fails during the iteration. This confirms the record by reading: the pinned C++ reports the
  unevaluated guess as found.
- **Rust port:** behaves as upstream by default. The fix is the same kind of change as d2757e1's
  N-D part (WR-253: "a best guess that cannot be re-evaluated is no solution"), so it goes under
  that flag, `Kernel::fixes.oscillating_skips_failed_guesses`, which `LocalSolveNd` receives via
  `set_skips_failed_evaluations` (`crates/solidcore/src/geom/local_solve_nd.rs`, #1995).
  Flagged as a follow-up by tdamsma on #1948.
- **Effect:** no geometric input is known to reach this path.
- **Tests:** `geom::local_solve_nd::tests::wr20_failing_first_evaluation_finds_no_solution_with_the_flag`
  (flag on: not found). `failing_first_evaluation_reports_found` still pins upstream's result with
  the flag off.
