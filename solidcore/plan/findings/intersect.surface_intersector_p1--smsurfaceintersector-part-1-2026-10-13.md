# intersect.surface_intersector_p1 (`SmSurfaceIntersector`, part 1, 2026-10-13)

- **Point arena.** `SmTsectPnt`s live in an arena of the Rust `SurfaceIntersector`
  (`alloc_point`, `TsectId`); the three intrusive `SmTList`s are `Vec<TsectId>`.
  Linking a point already on a list (undefined upstream: it corrupts both lists) is
  `InvalidInput` in Rust; the shim does not run such an `Add*`/`Assign` op and both
  sides report the op status `-2` (`ffi::REJECTED`). Later parts (`_p2` .. `_p5`) reach
  the lists through the `pub(crate)` fields.
- **Co-ported `SmSurface::IsClosed`.** The constructor needs `IsClosed(dom, SM_SP_U/V)`
  (unit `surface.surface_p7`, not ported). It is ported as a private helper
  (`surface_is_closed`, default tolerance, no continuity) together with the one-line
  wrappers `EvaluateDirectionalDerivs` / `Evaluate1stDerivatives` (`surface.surface_p1`).
  Statuses upstream ignores there keep the previous iteration's values (upstream's
  locals live across the sample loop); a failed `smgu_FrenetFromDerivatives` leaves a
  zero curvature vector, as upstream's partial write does. Replace the helper with the
  real `Surface::is_closed` when `surface.surface_p7` lands.
- **`SER` in boolean methods.** `IsPointOnCurve` / `sm_IsPointOnCurve` return the
  status of a failing call as `TRUE`; the port returns `Ok(true)` (upstream-issues
  WR-52). `AddStartPoint`'s "replace" branch is a no-op (WR-50);
  `IsPointOnStartPoint` compares squared distances with the tolerance (WR-51).
- **Difftest.** Programs over planes and B-spline surfaces (open patches, rational
  patches, closed tubes that pass `IsClosed`, self-intersection), sub-, cut, disjoint,
  degenerate and fillet domains, scales 1e-6 .. 1e5. `TestSpanAccuracy` leaves its
  deviation unset on an angle rejection; the shim reports `-777` (`ffi::UNSET`) there.
  Deferred: refinement of crossing/tangent-curve/singular points
  (`LocalSurfaceIntersect`), `StepBackFromSingularity` (`LocalPointSolve`),
  `sm_ComputeCurveType` off planes or with parallel normals, `IsPointOnCurve` in its
  10x-tolerance band (`SmSurface::DropPoint`) and bounded/exact start point pairs
  (`smsurf_EvaluateNormalSection`). `EvaluateLawPoint` (needs `SmFilletLaw` and
  `SmOffsetSurface`) has no cases: the shim cannot build an offset-surface intersector
  the Rust side can mirror.
