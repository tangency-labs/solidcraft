#### WR-456. The fillet point solve's bigger-step retry reuses the small step
- **Severity:** Wrong result
- **Area:** Blends
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::ComputePointValues` (`:3619-3627`): `double dBigStep = 1.0e-3;` then `StepSolve( rTsectPnt, dStep, ...)` again.
- **What happens:** when the first forward finite-difference step (`dStep < 1e-4`) finds no point, the "try a bigger step" branch declares `dBigStep` but passes `dStep` to `StepSolve`, so the retry repeats the failed attempt. `dStep = dBigStep` is only set afterwards, if the repeat succeeded.
- **Trigger:** a crossing point of a variable-radius blend, or a tangent point, whose first step of 1e-5 fails. Found next to #1173 (the per-point step); that case does not reach this branch.
- **How found:** reading the code while fixing tangency-labs/solidcore#1173.
- **Confirmed:** by reading; the Rust test records the guesses of the two solves.
- **Fix:** pass the bigger step to the retry.
- **Rust port:** `c6_compute_point_values` (`crates/solidcore/src/blend/fillet_solver/p4.rs`) behaves as upstream by default. With `Kernel::fixes.fillet_point_bigger_step_retry` the retry solves with `1e-3`. Test `bigger_step_retry_uses_the_bigger_step_with_the_fix` (the retry's guess lies 1x the first guess's distance from the point without the flag, 100x with it).
