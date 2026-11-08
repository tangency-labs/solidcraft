# curve.curve_p9: zero-end-tangent Hermite pair, loose tolerance (one case in 30,093)

`difftest --unit curve.curve_p9 --seeds 30000` (generator after the review-13 changes) has
one disagreement, `gen21507` (seed 7577090626449511304): `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`
(9), `SM_SR_ALL`, `tol` 0.5, two near-identical cubic Hermite curves (`p1` differs by about
0.1, `d1` identical, `d2` = (0,0,0) on both), first interval `[0.9424, 1.0934]`, second
`[0, 1]`. First solution value: C++ `6.019854387120155e-2`, Rust `6.026870704308737e-2`
(relative error 5.8e-2). The result is stable under clamping the first interval to
`[.., 1]`, and the case agrees bit-for-bit when `tol` is 0.01, the first interval is
`[0, 1]`, or `d2` is non-zero (`d1` or `(1e-3, 0, 0)`).

**Root cause: upstream reads an uninitialised Jacobian (undefined behaviour), not a porting
bug and not floating-point ordering.** Traced with gdb on the C++ side against a Rust trace
(local `eprintln!`, removed again):

- `SmMatrix::SmMatrix(rows, cols)` does not initialise its inline storage (`m_adMat`, used
  up to 6x6; only the heap case callocs). `SmLocalSolveNd::SolveIt` declares
  `SmMatrix sJacobian(lNumFun, lNumFun)` and uses it after the first `Evaluate` without
  checking that the evaluator wrote it.
- `SmCurve::LocalCurveIntersect` (here on the two `SmProjectedCurve`s, along `vec[0]`)
  runs a first Newton solve whose last accepted iterate is `(0.99998324, 1.0)`: a clamped
  step to `t = 1`, where the Hermite's end tangent is zero. Its second solve
  (`sLS2`, "start again at the solution") is started from that point. There
  `SmCCIntersectENFO::Evaluate` takes the tangent branch (gap squared < 0.01, tangents
  parallel within 0.001 degrees) and calls `SmCCTangentIntENFO::StaticEvaluate` without
  checking its status; that returns `SM_ERR` (squared tangent length below `SM_EFF_ZERO`)
  before writing `rF` or the Jacobian. `rF` keeps the solver's `SM_BIG_DOUBLE` fill, so
  `J * step = -F` is solved with whatever the stack slot of `sJacobian` holds.
- In C++ that slot still holds the Jacobian of the previous `SolveIt` on the same stack
  address (`[1.387e-7, -1.387e-7, -4.813e-2, 4.812e-2]`, from the first solve's
  iterate at `(0.94281, 0.94283)`). The resulting step lands at `(0.94237, 0.5)` and the
  iteration ends at `(0.94237, 0.94236)`, value `6.0199e-2`. Rust's `Matrix::new` zeroes
  the Jacobian, so the system is singular (`BadJacobian`), the second solve fails, the
  first solution `(0.99998, 1.0)` is kept and the later stages end at `(1, 1)`, value
  `6.0269e-2`.
- Every residual and Jacobian of the first solve, and of the second solve's first
  evaluation, matches C++ bit for bit (`F = [-1.0452749776845849e-12,
  2.3103247408282146e-3]` at the third iterate, same `x` values at every evaluation
  before the divergence), so the Hermite cache, the projected curve evaluation and the
  Newton code are not involved.
- Proof: stopping the C++ run in gdb at the second solve's first evaluation and zeroing
  `sJacobian`'s four doubles makes the same case agree with Rust exactly (both
  `[1, 1, 2, 2, 0, 1, 6.026870704308737e-2, 1, 1]`, PASS, relative error 0).

Not fixable without copying uninitialised stack garbage, so the port keeps the
zero-initialised matrix (`geom::local_solve_nd`, outside this unit) and `curve.curve_p9`
is unchanged. The failing shape is a zero-length end tangent at a clamped parameter plus
a gap below 0.1 and a loose tolerance, so it only shows in large seed counts; the
default 3,093-case set (which the gate and CI run) is clean, and the previous generator's
30,055-case run was clean.

**Handling (#94).** Logged as `UB-204` in `plan/upstream-issues.md`. The Rust side keeps zeroing
`Matrix::new`: emulating a stack slot is neither possible nor deterministic, and the port's goal is
deterministic results. The unit's generator skips the shape, so `--seeds 30000` no longer reports
`gen21507`: `reads_uninitialised_jacobian` in `crates/difftest/src/units/curve_curve_p9.rs` drops
op 9 cases where a Hermite end tangent of length zero is reached and the other curve is within 0.1
of that end. Measured over the default 30,093 cases it matches 4 (the failing one and 3
pairs that touch exactly at that end and pass); the other 38 cases with a zero end tangent are 0.5 or more
apart. It is deliberately narrower than "any zero tangent", which the generator keeps drawing
(8% of Hermite `d2`, 10% of `d1`) as degenerate coverage.
