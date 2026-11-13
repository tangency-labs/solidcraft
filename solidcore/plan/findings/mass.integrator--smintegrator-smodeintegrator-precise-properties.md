# mass.integrator (`SmIntegrator`, `SmODEIntegrator`, precise properties)

1528 default cases: 1520 pass, zero fail, eight deferred; maximum relative error
`4.979280456316938e-14`, without absolute floors. The C++ shim calls the actual
scalar/ODE integrators, edge precise properties and public length query, and
`SmFace::ComputePreciseProperties` on real rectangular/rotated plane faces,
disks, annuli and curved quadratic B-spline patches. For the face integration,
the actual input oriented trim curves, UV domain and bounding-box-derived area
estimate are exported from C++ and imported into Rust;
results and moments are computed separately. All 63 precise-property masks are
covered in both orientations, following `sm_CheckPrecisePropertySelection` in
`tests/SM_API_test/src/TestSmBrep.cpp:244`. Generated cases cover state-dependent
ODEs, scalar polynomial/sinusoidal/exponential functions, reversed intervals,
error scales, sub-intervals and small/large magnitudes. Values, adaptive steps,
mutated scales and accepted/reduced step counts are compared.

- **Review regression:** 155 additional cases cover full-circle trims and circular
  holes, both orientations, accuracies from `1e-3` to `1e-8`, tight/zero accuracies
  exercising numerical failures and retries, non-axis-aligned plane bounds, curved
  B-spline surface properties, and nested `AreaIntegrand::evaluate` compared with
  `SmSurface::Area` on rectangular sub-domains. Edge cases include exact rational
  quarter circles, non-unit weights, cusps, constant curves, zero-length intervals,
  multiple accuracies and `1e-9`/`1e9` scales. Before the fix, 16 circle-boundary
  cases failed (maximum relative error `1.4907788505308581e-4`); all now pass.
  `SmODEAreaIFEO::Evaluate` leaves its output untouched for a zero U tangent;
  the adapter now seeds scratch from the particular integrator derivative array,
  preserving zeroed stage buffers and the main loop's previous derivative separately.
  No tolerance or absolute floor changed.
- **Upstream behavior retained:** `RungeKuttaODEIntegrate` starts its accumulator
  at zero regardless of `ystart`, except its early return (equal endpoints or
  first step equal to `SM_EFF_ZERO` to scaled tolerance), which leaves it alone.
  `IntegrateIt` ignores its algorithm selector and returns success with zero on
  ODE/evaluator failure; see WR-94 in `plan/upstream-issues.md`.
- **Safety:** zero-based component slices and closures replace one-based array
  subclasses. Dimensions are bounded at 4096, the step budget at 1,000,000 and
  rejected-step retries at 1024. Active integrations reject NaN controls and
  non-finite evaluations; no input-driven recursion. Infinite accuracy/scale floors and unused non-finite
  initial values agree with C++ in a direct harness test; equal endpoints return
  before checking unused controls. Scalar failure masking never masks `Unported`.
- **Deferred:** B-rep bounding boxes (`query.brep`, 3), face bounding boxes
  (`query.face_p1`, 2), B-rep enumeration (`topology.brep_p1`, 2), and thick-face
  offset evaluation (`surface.offset_surface`, 1). Oriented trim construction
  still calls `UNPORTED(topology.face_p1)` in the topology-facing face entry;
  callers with oriented boundaries can use `boundary_properties` directly.
- **Back-edges:** all three `UNPORTED(mass.integrator)` sites are wired: curved
  lengths in `curve.curve_p5`, area strips in `surface.surface_p1`, and the inner
  precise-property integration in `topology.face_p2`.
