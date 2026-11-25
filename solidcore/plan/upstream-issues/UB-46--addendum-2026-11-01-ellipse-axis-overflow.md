Updates: UB-46

#### UB-46 (update). An ellipse axis whose squared length overflows leaves `SmEllipse` without a NURB

- **Trigger:** `SmEllipse(C, X, Y, domain, rx, ry, 3, ...)` with an axis component of about 1.34e154 or more, so that `X.Dot(X)` or `Y.Dot(Y)` is infinite. `SmAxis2Placement::SetCanonical` unitizes the axis to a zero vector (WR-159). The constructor's `SE(MakeNurb())` then leaves the curve without a NURB, which UB-46's class of queries dereferences. Nightly fuzz case `topology.trimming_tools` fuzz00586 (`--fuzz 600 --seeds 40`; base `gen0322`, mutation `/curves/0/ellipse/y/0 = -1e+300`, op `order`) crashes the C++ reference. The kernel log gives `segfault at 10 ... error 4`, a read of offset 0x10 through a null pointer, at an address that `nm` resolves to `SmBSplineCurve::EvaluatePoint(double, SmVector3d&) const + 0x38`.
- **Boundary, measured** (the C++ side alone, `--fuzz-exec`, `y[0]` of that case varied; `x[0]` once):

  | axis component | `Dot` | C++ |
  |---|---|---|
  | `y[0]` = -1e300, -1e200, and `x[0]` = 1e200 | infinite | SIGSEGV |
  | `y[0]` = -1.3e154, -1e150 | finite | runs |

  `y[0]` = -1.4e154 (infinite `Dot`) was already refused by the validator's independence test.
- **Why the guard missed it:** `topology.trimming_tools`' ellipse validator tests independence as `|X×Y|² < 1e-6 |X|²|Y|²`. With an overflowing axis both sides are infinite and the test passes.
- **Guard:** the validator refuses an ellipse whose `X` or `Y` has a squared length that is not finite (`ellipse: axis length overflows`), as the circle, sphere, torus and extrusion validators already do (`query_loop`, `surface_sphere`, `surface_torus`, `surface_surf_of_extrusion_p1`). The case is in `crates/difftest/tests/fuzz_corpus/topology.trimming_tools.json`.
- **Rust port:** not affected: the fuzz case returns without a panic.
