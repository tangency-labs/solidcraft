# surface.surface_p1 (`SmSurface` part 1 and `SmSTEPSurface`, 2026-10-14)

- **Shim guard.** The base `GetDomainWithContinuity` dereferences a `NULL` surface cache
  when none can be built (upstream-issues UB-83). The shim checks
  `smsurf_GetSurfaceCache` first and reports `SM_ERR`, which is what the Rust port returns
  (the cache error). The check builds and stores the cache, so the call that follows uses
  the same cache.
- **B-spline back-edges.** `SmBSplineSurface::IsDegenerateCurve` and
  `IsDegeneratePoint` (sub-domains) pass the call to the `SmSurface` base body; the
  B-spline methods run it on a copy wrapped as a `Surface`, which has no owner (the owner
  is held by the `Surface` wrapper). `CalculateBoundingBox` grows its boxes by
  `GetFace() ? face tolerance : SM_ZONE_TOL_3D`: a bare B-spline uses `SM_ZONE_TOL_3D`,
  and `Surface::calculate_bounding_box` returns `Unported(topology.face_p2)` first for a
  B-spline or plane with a face.
- **trim_srf_cache face cases.** They used to defer through the `GetFace` stub of the
  B-spline bounding box. With it wired, the Rust side ran without a face and returned
  `SM_ERR_NULL_POINTER` where the C++ side had a brep face. The runner now defers them
  on `SmBrep::CreateFaceFromSurface` (`topology.brep_p8`), the step the Rust side lacks.
- **Not wired**, still `UNPORTED(surface.surface_p1)`: `Grid::expand_hierarchy` (needs a
  surface handle type for the grid elements), the surface cache's
  `CalculateBoundingBox` of curve-bounded and STEP surfaces (called while that cache is
  being built), and the `SmIsoCurve` reader of non-B-spline surfaces (that curve still
  holds a B-spline part). `CrvOnSurf` now uses the concrete `Surface` reader dispatcher;
  its former B-spline-only reader restriction is removed.
- **`CreateSilhouetteCurves` (2026-10-21).** Ported in full; the `Sil` difftest op
  compares every 3D/UV curve bit for bit (27 of the 34 `my_test_sil_adv` faces; 7 defer
  on `SmSurface::DropCurve`). Two things a later agent should know: (1) a generated
  B-spline at `1e-4` scale with a collapsed row made the Rust surface cache stop at
  `MAX_SUBDIVISION_NODES` (`InvalidInput`, 1007) where the C++ returned success with no
  curves (the documented `surface.surface_cache_p3` bound, not a port error); the
  generator skips surfaces smaller than `1e-2`. (2) The Rust trace is 100-250x slower
  than the C++ (`sil_adv_28_toruswvy`: 0.06 s C++, 13.9 s Rust): `perf` puts ~70 % in
  NURBS curve evaluation and fresh `CurveCache` tree builds, i.e. the per-call caches
  (upstream keeps them on the object), not the silhouette code.
