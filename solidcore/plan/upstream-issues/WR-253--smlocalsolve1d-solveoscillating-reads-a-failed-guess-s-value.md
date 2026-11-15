#### WR-253. `SmLocalSolve1d::SolveOscillating` reads a failed guess's value as a sign change
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmLocalSolve1d.cpp:470-495` `SmLocalSolve1d::SolveOscillating`: a failed `Evaluate` of an interior guess `continue`s (`:478`), and the sign test after the loop (`:495`) reads `sTempIter.m_dFOfT` from the last guess whether or not its evaluation succeeded.
- **What happens:** when the last guess (0.99 of the way) fails, its `F` is whatever the evaluator left: an evaluator that zeroes its outputs before failing, as `SmCurvePropertyEFO::Evaluate` does (`SmCurve.cpp:14918`), leaves 0. `0 * F(curr) <= 0` passes as a sign change, the guess is bracketed with `F = 0`, and bisection returns it as converged (`SM_TR_FOUND_ANSWER_CONVERGED`) although `F` there is not zero.
- **Trigger:** an oscillating `SolveIt` whose evaluator fails at the last interior guess. In SolidCraft's rig, the radius-of-curvature test of `SmBSplineCurve::AssertValid` (once #475 is fixed) on a straight UV trim: box minus a y-axis cylinder tangent to the box bottom (`smlib_test_booleans` tangent_cylinders, seed 74 op 1) gives two "minima" of radius 0 (tangency-labs/solidcore#481).
- **How found:** issue #481; traced at `c9c5979`.
- **Confirmed:** no (the port reproduces it; no C++ run here).
- **Severity:** wrong result (a solution where there is none).
- **Rust port:** Faithful to the code: `LocalSolve1d::solve_oscillating` keeps the failed evaluation's value and brackets it the same way (test `failed_guess_is_read_as_a_sign_change_as_upstream`: `F = t^3 - 2t + 2` reports converged at the failed guess, where `F` is about 0.95, and nothing without the failure). Not changed, so the unit keeps the reference build's results.
