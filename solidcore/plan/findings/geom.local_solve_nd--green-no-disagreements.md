# geom.local_solve_nd (green; no disagreements)

3043 default cases (43 fixture, 3000 generated), bit-identical, 0 deferred; a 30,043-case
run was also clean. Both sides solve the same test equation set (shim `ScTestENFO`, runner
`TestEquations`) and the whole `Evaluate` trace (points, Jacobian requests, failures) is
compared. Upstream behaviour worth knowing:

- **A failed first `Evaluate` is reported as found**: `SolveIt` returns `SM_SUCCESS` with
  `rbFoundSolution = TRUE` and `SM_TR_UNABLE_TO_CONVERGE`, the solution being the guess.
  Kept as is.
- **Uninitialised state.** `bFoundAnswer` is read after `Evaluate` even when an evaluator
  never writes it (the header notes many do not), and `m_lCurrentIteration` is not set by
  the constructor (early returns leave it untouched). The port returns "found" from
  `evaluate` (`Ok(bool)`; a failed call leaves the flag unchanged) and starts the counter
  at 0; the shim sets `m_lCurrentIteration = 0` and its evaluator writes `rbFoundAnswer`
  only on success.
- **Line-search break on a failed unclamped evaluation** keeps the previous `dNextSumF`
  and makes the *unclamped* next guess the current point; when the scaled clamped step
  wins, `sJacobian2` is discarded (the next iteration uses the unscaled point's Jacobian).
  Ported faithfully.
- **`SolveLinearSystem` statuses other than `SM_ERR`** (e.g. `SM_ERR_INVALID_INPUT` if an
  evaluator resized the Jacobian) do not stop the iteration upstream and leave the step
  stale; the port returns them as errors (unreachable with an `N x N` Jacobian).
- **Periodic clamping cost.** `SmExtent1d::PeriodicWrap` steps one period at a time, so a
  diverging Newton step (e.g. 1e8 with a period of 0.01) loops ~1e10 times in both
  languages. The generator only uses periodic intervals of width >= 0.5 on well-posed
  problems.
- `m_dFoundAccuracy` is never written by `SolveIt` and `m_eLocalSolverType` is never read;
  neither is ported. Wired `SmTol::GetScaledZero(const SmEvalNFunctionsObject&)` in
  `geom::tol` (`scaled_zero_of_equations`).
