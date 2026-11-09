# intersect.surface_intersector_p4

- **No virtual dispatch in the intersector parts.** `SmSurfaceIntersector`'s
  `TraceCurve` / `DoPointIntersection` reach `ComputeNextPoint`, `ComputePointValues`
  (which calls `ComputeSingularityPoint` for parallel normals), `FlushCurve` and
  `TestSpanAccuracy`, all virtual upstream. Parts 1-4 port them as methods of the basic
  `SurfaceIntersector`, so they always run the basic versions. `SmAdvSurfaceIntersector`
  overrides `ComputeSingularityPoint` (and `SmFilletIntersector` several more), so the
  part 4 functions cannot replace the advanced intersector's `TraceCurve` stub
  (`intersect/adv_surface_intersector.rs`) or `SmSurface::PointBasedSurfaceIntersect`'s
  `DoPointIntersection` (`surface/surface/p2.rs`, run on an `SmAdvSurfaceIntersector`).
  Those stubs stay `UNPORTED(intersect.surface_intersector_p4)` until the parts take a
  trait for their virtual calls.
- **Real-value coverage is narrow until `surface.surface_p11`.** The basic intersector
  refines every crossing, tangent-curve and singular point with
  `SmSurface::LocalSurfaceIntersect`, and the basic `ComputeSingularityPoint` fails
  (`SM_ERR`) for parallel normals. So on planes the traces that run without deferral
  start at raw points of the types it does not refine (unknown, tangent, coincident,
  touching), and `DoPointIntersection` on coincident or parallel planes fails in its
  `ComputePointValues` (`SM_ERR`, compared with all evaluated point values).
  `FindInteriorCurves` always defers (global surface solve).
