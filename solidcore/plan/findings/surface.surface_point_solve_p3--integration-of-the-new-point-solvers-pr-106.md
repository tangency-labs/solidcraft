# surface.surface_point_solve_p3 — integration of the new point solvers (PR #106)

- `curve.iso_curve` now stores a concrete `Surface`, so boundary solves retain the
  analytic `SmPlane` evaluator and the original surface-of-revolution / STEP
  dispatch. The previously documented B-spline-only dispatch gap is removed;
  missing *surface* operations still propagate their owning unit's `Unported`.
- Temporary solver copies retain their surface owner and the exact NURBS data of
  B-splines, planes and surfaces of revolution. `Surface::clone()` is the upstream
  copy constructor: it resets the owner and can fix knots / construct a plane's NURB,
  so it cannot stand in for `SmTemporaryChangeValue` during a solve.
- `CheckCrease` is disabled by upstream's `constexpr sbDoCreaseSubdomains = FALSE`.
  The port deliberately returns `(false, false)` without evaluating a crease.
- The local driver retains the 35-iteration limit and the separate status, found
  flag, termination reason and iteration count. `FindNextStep` also preserves the
  upstream plane-converged branch assigning `sNREval`, rather than silently changing
  the C++ algorithm.
- `Surface::local_point_solve`, `Surface::drop_point` and the patch-local point
  solver take the caller's kernel. The context is threaded through their geometry,
  intersection and topology callers, including `SurfaceEvaluate` wrappers; local
  and global fallbacks no longer replace caller tolerances with defaults.
- Boundary regression `matrix_0363` exposed an overly strict Rust guard in
  `SurfOfRevolution::evaluate_step`: release C++ accepts sweep-only derivative
  orders such as `(2, 0)`, despite its debug-only equal-order assertion. These
  outputs are fully evaluated and required by `SmIsoCurve`. The guard now matches
  release C++; other unequal orders retain `SmVector3d`'s default `UNDEF` values
  for generator derivatives beyond `min(hu, hv)`, as upstream does.
- Evaluation copies of nested STEP, curve-bounded and volume surfaces preserve
  exact nested geometry and ownership; a preflight walk bounds every owned
  surface, trim-curve and volume-map branch to 64 levels before recursive copying.
- **PR #106 review:** the seam jump uses `!new_uv.approx_ne(uv)`, preserving the
  left operand and the scaled tolerance of `SmVector2d::operator!=`. Two native
  regressions failed before the fix (`cpp U=0`, `rust U=1e-6`) and pass after it;
  four controls just beyond the tolerance still jump as upstream does. The large
  V coordinate makes the short U seam tolerance-equivalent without violating the
  native harness's minimum parameter-range guard.
- Six seam-threshold fixtures and 36 STEP-wrapped sphere-, torus- and cone-shaped
  revolution fixtures cover both seam sides, poles and three solver operations.
  These use actual `SmSurfOfRevolution` generators, not B-spline stand-ins.
- Integrated the final p4 back-edges from merged PR #49 and the latest p5
  back-edges from PR #50. The remaining `heal.gap` surface drop now calls the real
  `Surface::drop_point`, preserving its optional guess and ignored ordinary status.
  No `UNPORTED(surface.surface_p4)` or `UNPORTED(surface.surface_p5)` sites remain.
- Reaching `sm_SpLocalSolve` through p6 uncovered a stale native shim sentinel:
  its adapted `sm_SCLocalSolve` returned `SC_SP_LOCAL_SOLVE` instead of calling
  the reference solver. p5/p6 now include the same verbatim `sm_LoadGuesses`,
  `sm_PassSPConvergenceTest` and `sm_SpLocalSolve` bodies from a shared shim header.
  No reference numerical logic or differential tolerance is changed.
- **After #50 merged:** merged `origin/main` (`51ec6119`) and retained the caller's
  kernel in `heal.gap` while preserving `DropPoint`'s failure defaults (`false`,
  the supplied guess or `UNDEF`, and `BIG_DOUBLE`). Full local CI passed: 1152
  workspace tests; native difftest 169 units (92 green, 77 partial), 410264 passing
  cases, 11837 deferred, zero failures. The p3/p5/p6/heal.gap perturbation checks
  detect every compared case.
- The exact temporary copy remains necessary to preserve the immutable surface's
  out-of-bounds flag on all exits. Its owned-geometry preflight still bounds copy
  depth to 64. A borrowing evaluation adapter would be a separate optimization.
