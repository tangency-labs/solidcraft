#### UB-204. `SmMatrix(rows, cols)` leaves its inline storage uninitialised, and `SmLocalSolveNd` solves with it when an evaluator fails
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `SmMatrix.h:39` (constructor, "leaves mat values uninitialized"; inline storage `m_adMat[36]` at `:30`), `SmLocalSolveNd.cpp:115` (`SmMatrix sJacobian(lNumFun, lNumFun)`, used by `SolveIt` whatever the evaluator did), `SmCurve.cpp:16680` (and `:18834`): `SmCCIntersectENFO::Evaluate` (def. `:16589`) calls `SmCCTangentIntENFO::StaticEvaluate` and ignores its status.
- **What happens:** When the curves are within 0.1 of each other and an evaluator fails before it writes `F` and the Jacobian (here: a tangent of length below `SM_EFF_ZERO`), `SolveIt` still solves `J * step = -F` with the matrix it never filled. In a 6x6-or-smaller system that is the stale stack slot of an earlier solve, so the step, and the solution it leads to, depends on what ran before.
- **Trigger:** `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE` (9), tolerance 0.5, two near-identical cubic Hermite curves whose end tangent `d2` is zero, the first interval `[0.9424, 1.0934]` (a Newton step clamped to `t = 1`).
- **How found:** generated difftest case `gen21507` (`curve.curve_p9`, `--seeds 30000`), traced with gdb: zeroing the four Jacobian doubles at the second solve makes C++ and Rust agree bit for bit.
- **Confirmed:** yes (that case).
- **Severity:** crash/UB (an uninitialised read; the result is nondeterministic across call histories).
- **Rust port:** `Matrix::new` zeroes its storage, so the system is singular, the solve reports `BadJacobian` and the previous solution is kept: deterministic, and the safe reading. The curve_p9 generator does not hand this shape to the C++ reference (`reads_uninitialised_jacobian`). See `plan/findings.md`, "curve.curve_p9: zero-end-tangent Hermite pair, loose tolerance".
