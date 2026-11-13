# geom.polynomial (green; no disagreements)

6038 default cases (38 fixture from `my_test_polynomial` + edge cases, 6000 generated)
agree bit for bit with upstream (`max_rel_err = 0`). Upstream behaviour worth knowing:

- **Uninitialised read in `SolveQuarticEqn`.** In the `r != 0` branch it takes
  `z = adCSol[0]` from the resolvent cubic (leading coefficient `1.0`) without checking
  the root count. With `dZeroTolerance > 1` that cubic degrades to a quadratic that may
  return no roots, leaving `adCSol[0]` uninitialised (UB). The port uses `z = 0.0`; the
  FFI wrapper rejects quartics with `tol > 1`, so generated quartic tolerances stay <= 1.
- **Unused output slots.** `SolveQuadraticEqn` zeroes both slots; `SolveCubicEqn` /
  `SolveQuarticEqn` leave slots past `rlNumSolutions` untouched. Callers must read only
  the first `rlNumSolutions`; the port's `Roots` exposes exactly those.
- `SolveCubicEqn`'s three-root branch uses the plain `sqrt` for `sqrt(-cb_p)` but
  `smos_Sqrt` (clamping) for `sqrt(-p)`, and `smos_ArcCosine` clamps to `[0, PI]`; ported
  as is. Every solver always returns `SM_SUCCESS`, so the port is infallible.
