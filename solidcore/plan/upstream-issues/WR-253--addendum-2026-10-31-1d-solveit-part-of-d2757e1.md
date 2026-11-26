Updates: WR-253 (and WR-20)

#### Updates: WR-253 (2026-10-31). The last part of d2757e1: the 1-D `SolveIt` with no interval

- **Upstream:** `source/SMLib/src/SmLocalSolve1d.cpp`, `SmLocalSolve1d::SolveIt`, the failure
  branch of the first `Evaluate` (~805-845). With an interval it tweaks the start by 1/1000 of the
  interval and returns `SM_ERR` if that fails too. Without an interval it falls through and iterates
  from `m_vCurr`, which holds no function value: an evaluator that zeroes its outputs before
  failing (`SmCurvePropertyEFO::Evaluate`) gives `F = 0`, which reads as converged, and the guess is
  reported as a root. This is the 1-D counterpart of WR-20 (the N-D solve reports an unevaluated
  guess as found).
- **Fork fix:** tangency-labs/smlib@d2757e1 ("the local solvers take no value from a failed
  evaluation"): an `else { return eStat; }` on that branch, "as the interval case does when its
  tweaked start fails too". Its `SolveOscillating` part is this record's #481 addendum, its N-D part
  the 2026-10-31 N-D addendum (#1948, #1995); this was the part left (#2009).
- **Rust port:** behaves as upstream by default. Under the same opt-in flag,
  `Kernel::fixes.oscillating_skips_failed_guesses` (passed to the 1-D solver by
  `LocalSolve1d::set_failed_guess_no_sign_change`), `LocalSolve1d::solve_it` without an interval
  returns the evaluator's error (`crates/solidcore/src/geom/local_solve1d.rs`).
- **Effect:** none known. Every `LocalSolve1d` in the crate is built with `with_interval`,
  so no geometric input reaches this branch.
- **Tests:** `geom::local_solve1d::tests::d2757e1_failed_first_evaluation_without_interval_fails_with_the_flag`
  (flag on: the evaluator's `SM_ERR`; a start that evaluates is unaffected) and
  `failed_first_evaluation_without_interval_is_a_root_as_upstream` (flag off: `t = 1` is reported
  as a root of `t^2 - 2`, as upstream).
