# curve.curve_p7: flat-circle report stability (#357) (2026-10-25)

Updates: `plan/findings.md`, "## curve.curve_p7 (`SmCCIGlobalSolver::LocalSolve`, `SmCurve::GlobalPropertyAnalysis`)"

- **Flat-circle report stability (#357).** Exact rational circles have constant
  radius of curvature; rounding noise can select different extrema counts on
  different machines even when C++ and Rust agree locally. The generated circle
  family now uses a noncircular rational ellipse (homogeneous X coordinates scaled
  by exactly two) only for Minimize/MaximizeRadiusOfCurvature (13/14). Circle-origin
  provenance is carried through generation; no random draws, seeds, operation
  choices, validation rules, solver implementation or comparison tolerances change. Seven
  of 3000 generated inputs change; the remaining 2993 and all 66 current fixtures remain.
  The original gen1409 (seed 7577092825472747628) is retained in
  `crates/difftest/tests/data/curve_p7_flat_circle.json` as a complete ordered native
  parity regression, including solution count, outside autodiscovered report
  fixtures. Its platform-dependent extrema count is deliberately not frozen.
  Separate analytic ellipse controls check radii 1 and 8 for semiaxes 4 and 2;
  seven frozen generated-case output sizes also test report shape across macOS
  and Linux without weakening the full native comparison. This addresses the
  curve half only; the separately merged polar-box work (#361) remains intact.
