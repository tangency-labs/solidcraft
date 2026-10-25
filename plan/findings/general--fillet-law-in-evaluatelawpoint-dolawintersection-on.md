# Fillet law in EvaluateLawPoint; DoLawIntersection on SmFilletIntersector (PR #658, 2026-10-23)

- **Law.** `SmSurfaceIntersector::EvaluateLawPoint` now evaluates the `SmFilletLaw`
  (`&mut`: `Evaluate` caches a B-spline law's extension through a `const` cast) and sets
  the offset distances of the intersector's shared surfaces (`OffsetSurface::set_offset_distance`
  takes `&self`, so the old "needs `&mut`" dispatch gap was stale).
- **Virtual dispatch.** `DoLawIntersection`, `SplitSpan` and `ComputeLawPointValues` are
  generic drivers over `IntersectorVirtuals`, which gained `EvaluateLawPoint`,
  `ComputePointValuesDelta` and `TestSpanAccuracy` (defaults: the basic bodies).
  `FilletIntersector::do_law_intersection` runs them on the `SmFilletIntersector`
  overrides; `SmVariableRadiusFS::ReCalcFilletGeom` uses it, with
  `impl FilletSolverOps for VariableRadiusFs`. On the basic intersector all 8 recalc cases
  that reach the law mismatched (statuses equal, points different): upstream never
  evaluates the law there (`SmFilletIntersector::EvaluateLawPoint` solves the solver's
  point on the plane instead).
- **Two aliasing effects upstream relies on** (found with a C++ probe that subclasses
  `SmVariableRadiusFS` and prints every `LoadJacobian`/`PointOnPlaneSolve`):
  1. `DoLawIntersection` passes `pFirst->UVPos(0)`/`UVPos(1)` to `ComputeLawPointValues`
     as `const&` guesses of the point it computes. After a try whose
     `ComputePointValuesDelta` fails, the point's UVs (written by `ComputeSurfaceValues`
     from an unsolved `SmTsectPnt`, `1.23456e-61`) are the next try's guesses; from that
     guess the plane solve converges where the original guess does not. Modelled by
     `LawGuess::PointUvs` (re-read every try). Case gen0106 of
     `blend.fillet_standard_solver_p2`: C++ status 1007, Rust 1001 before the fix.
  2. The intersector's surfaces are the solver's own `SmOffsetSurface`s, so base
     extensions made by the intersector (its constructor evaluates the extended domain)
     are the solver's too, and vice versa. Rust's trace surfaces are copies: the extension
     made on the copy never reached the solver's rail, which later made a smaller one of
     its own; evaluations then differed in the last ulp (`LoadJacobian` rF[2]
     -2.9963362048811302 C++ vs -2.9963362048811324 Rust), and a distance between nearly
     coincident end points differed (gen0173: curve parameter 1.42e-15 vs 0). The fillet
     solver p4 `IntersectorSolver` now carries extensions both ways
     (`OffsetSurface::follow_extension_of`: before each solver call the rails take the
     trace surfaces' current extension, after it the trace surfaces take the rails'),
     besides the offset distances it already synced. Every blend unit and the surface
     intersector units re-ran unchanged or improved.
