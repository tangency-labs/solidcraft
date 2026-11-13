# intersect.surface_intersector_p2 (`SmSurfaceIntersector`, part 2, 2026-10-14)

- **Gap walk bound.** `ClassifyBoundaryStartPointIntersection` walks the gap function
  with steps growing by 5 until a sample leaves a domain. With a zero UV tangent on both
  surfaces the samples never move until `delta` overflows to `inf`; then `0 * inf` is
  NaN and `ContainsPoint2d` treats a NaN point as inside, so upstream never ends. The
  port stops with `SM_ERR` after `MAX_GAP_SAMPLES` (2000) samples. The shim's
  `is_rejected_classify` rejects every point with both UV tangents zero (or a non-finite
  UV value), so the early returns before the walk (tangent point, both steps leaving the
  domain, tangent-curve / singularity / coincidence points off a tangent natural
  boundary) are not compared for such points. Narrowing the guard to the walk itself
  would need the shim to replay the pre-walk classification; not done. A review run
  with the zero-tangent part of the guard disabled finished normally (46 more classify
  ops agreed, 9 more deferred, the C++ never hung on those inputs), so the guard is
  wider than needed; the pre-walk branches are already covered by points with non-zero
  tangents, so this costs input coverage, not branch coverage.
- **DoBoundaryPlaneTest Tangent/Coincident unreachable.** The interval branch only
  produces `SM_TC_CROSSING` in the difftest: with the base intersector, endpoint point
  types of `TangentCurve` or higher cannot be reached (`ComputeSingularityPoint`
  returns `SM_ERR`), so the Tangent and Coincident classifications are uncompared. They
  are unreachable here, not skipped by a guard.
- **Re-run when dependencies land.** `ComputePointValues` step 7 (the "found" branch
  and the else branch) and the gap walk past its first in-domain sample are fully
  deferred on `surface.surface_p3` and `surface.surface_p5`; re-run this unit's
  difftest when those units become green/partial.
- **NaN curve intersection tolerance.** `DoBoundaryPlaneTest` with a zero tolerance and
  an empty box divides 0 by 0. The C++ and the Rust agree on the result (fixture
  `adv_graze_nan`), so the generator no longer excludes it.
- **FlushCurve not guarded.** Curve parameters that do not increase reach
  `SmBSplineCurve::CreateCanonical`, whose `N_IgesValidate` rejects decreasing knots
  with `SM_ERR` (equal knots pass); both sides agree, so the shim runs every `Flush`
  (the earlier `is_rejected_flush` hid about a fifth of the generated flushes and the
  whole `sm_FlushCrv` error path).
- **Input bound.** The FFI accepts finite inputs up to `MAX_COORD = 1e9` (as other
  units do), so huge-scale inputs beyond 1e9 are not compared.
- **EvaluateNormal wired.** With `surface.surface_p7` partial on `main`,
  `surface_evaluate_normal` now calls `Surface::evaluate_normal` (B-spline, plane and
  curve-bounded over either basis); the 77 cases previously deferred on it are compared
  and pass. `SmCurveBoundedSurface` does not override `IsSingularity`, so a curve-bounded
  surface runs the base `SmSurface::IsSingularity` and stays deferred on
  `surface.surface_p5` (correctly, not an over-deferral).
