# geom.local_solve1d (green; no disagreements)

3316 default cases (316 fixture + 3000 generated) agree bit for bit with upstream
(`max_rel_err = 0`), comparing the full evaluation trace, outputs and solver state.
Upstream behaviour worth knowing:

- **Interval-less constructor is broken.** `SmLocalSolve1d(rFunctionEvaluator)` leaves
  `m_vInterval` as `SmExtent1d()` (min `1e20`, max `-1e20`), yet `SolveIt` still limits
  each Newton step to `GetLength() * 0.08 + SM_EFF_ZERO`, which is negative: every step
  becomes `+-1.584e19` *against* the Newton direction. Ported as is (fixtures
  `no_interval_*`); callers should always pass an interval.
- **Uninitialised `bFoundAnswer`.** When the first evaluation in `SolveIt` fails and
  there is no interval (no retry), the serendipity check reads `bFoundAnswer` as the
  evaluator left it (uninitialised if it did not write it). The shim's test evaluators
  always write it on failure; the port starts it `false`.
- **Partial evaluator writes are observable.** `SolveOscillating` keeps scanning after a
  failed evaluation and then reads `sTempIter.m_dFOfT`, i.e. whatever the failed
  evaluation wrote (or the previous value). The Rust `EvalFunction::evaluate` therefore
  keeps upstream's out-parameter shape and the solver keeps partial writes.
- `SolveIt` ignores the `SolveByBisection` status after the Newton loop and leaves
  `m_dLowerBracketT`/`m_dUpperBracketT` (and `m_dFoundAccuracy` on the Brent paths)
  from the previous solve; the port keeps that state.
- Not reached by any case (no difftest coverage, ported by inspection): the
  bracket choices in `SolveOscillating` that pick the *current* iterate (the cycle phase
  always puts the guess on the previous iterate's side in the constructed cycles), a
  failing final evaluation in `SolveOscillating`, a failing `SolveByBrent` called
  directly from the Newton loop, and `SolveByBrent`'s `u == x` early return.
