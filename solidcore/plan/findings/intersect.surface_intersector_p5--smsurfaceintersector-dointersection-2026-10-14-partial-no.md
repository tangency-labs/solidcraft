# intersect.surface_intersector_p5 (`SmSurfaceIntersector::DoIntersection`, 2026-10-14; partial, no disagreements)

- **Dispatch.** Every upstream caller runs `DoIntersection` on an
  `SmAdvSurfaceIntersector`, so it is ported on `AdvSurfaceIntersector` (its
  `SolveBranch`/`LocalSolve` overrides answer the `SM_SO_INTERSECTION_TEST` solve, its
  `FindInteriorCurves` override finds the loops). DISPATCH GAP for the porters of
  `surface.surface_p8` / `surface.surface_p11`: `TraceCurve` (part 4) calls the basic
  `ComputePointValues` -> `ComputeSingularityPoint` (`SM_ERR`), not the advanced
  override, so a traced point with parallel normals will disagree once start points
  can be found. Today every start point needs `SmSurface::GlobalCurveIntersect`
  (`surface.surface_p8`) or `SmSurface::LocalSurfaceIntersect` (`surface.surface_p11`),
  so no compared case reaches it.
- **Surface caches.** The real cache build needs `SmSurface::CalculateContinuities`
  (`surface.surface_p6`): every case without hand-built trees defers there. The runner
  swaps hand-built trees into the real C++ caches (`m_pTree`, restored after the call)
  and gives the Rust side caches holding the same trees. `SmCurveBoundedSurface`s
  without boundary curves get no C++ cache (`NER` -> `SM_ERR_NULL_POINTER`); they run
  only without trees.
- **Plane iso curves.** `SmPlane` boundary planes need the cache's
  `SmPlane::CreateIsoParametricCurve` (self-tagged `surface.surface_cache_p1` gap: the
  cache stores iso curves as B-splines, a plane makes `SmLine`s), so plane scenes that
  reach `DoBoundaryPlaneTest` defer; the generator mostly uses the same planes as
  bilinear B-splines, which compare.
- **Back-edges.** `base_global_surface_intersect` (surface_p2; plane_p2 and
  surf_of_revolution_p2 fall back to it) and `CreateParallelProjectionCurves`
  (surface_p7, whole body ported: it now returns `None` UV curves for `NULL`
  entries and takes the kernel) call it; their deferrals move to `surface.surface_p6`
  (surface caches). `surf_of_extrusion_p2`'s fall-back stays a stub tagged
  `intersect.adv_surface_intersector` (its output type differs).
- **Review (PR #82).** 94 adversarial fixture cases
  (`fixtures/intersect.surface_intersector_p5/review_adversarial.json`: zero, negative,
  huge and corner-gap tolerances, angle tolerances, collapsed corners, all 16 corner
  pairings, sector-boundary epsilon, sub/extended/reversed domains, scales 1e-7..1e7,
  rational bilinears, self intersection, a point-degenerate surface): 47 pass
  bit-identical, 0 fail, 47 defer. Never compared: the trace loop, the backward
  re-trace and the degenerate-curve sweep (every start point needs `surface.surface_p8`
  / `surface.surface_p11`). When those land, recheck the DISPATCH GAP above together
  with the fact that `DoIntersection` ignores `TraceCurve` errors: the gap would then
  silently turn a curve into a through point rather than fail. Open follow-ups:
  `surf_of_extrusion/p2.rs` still stubs `DoIntersection` under the (already partial)
  `intersect.adv_surface_intersector` tag and should be wired; `do_intersection_with_caches`
  leaves `point_test_enabled = false` / `process_boundary_curves = true` on the
  caller's caches (upstream restores them; the solver borrows the caches for `'t`,
  so restoring needs an API change); `FindBoundaryStartPoints` runs on a full clone
  of the caches (trees included), a cost only. The upstream prog_tests
  (`my_test_ssi`, `my_test_parallel_projection`, ...) are not fixtures yet: they
  would all defer on the real cache build (`surface.surface_p6`).
- **Shared cache in `GlobalCurveIntersect` (PR #118, surface.surface_p8 wired).** Wiring
  `SmSurface::GlobalCurveIntersect` first gave 53 failures (rec lengths 17 vs 87 and 99 vs
  547, control points off at 1e-7 relative, deviations at noise level), all on hand-tree
  cases of bilinear surfaces: upstream's `GlobalCurveIntersect` (and the
  `GlobalLineIntersect` it calls for a straight boundary curve) fetches the other surface's
  cache with `smsurf_GetSurfaceCache`, which returns the one cache `DoIntersection` holds,
  hand tree included; the port built a fresh cache with the real tree, so the line and tree
  solves ran on another decomposition. `FindBoundaryStartPoints` now passes the caller's
  cache (`Surface::global_curve_intersect_in`, on a copy whose handles name the curve and
  surface objects of the solve, `SurfaceCache::map_handles`; the curve object carries its
  natural interval for `IdenticalSolutions`), and `do_intersection` /
  `find_boundary_start_points` name their caches' surfaces (leaf solves read the tree
  owner). Without hand trees the result is unchanged (the shared cache's tree and flags
  equal a fresh build's). After the fix: 1725 cases, 1009 pass, 0 failed, 716 deferred;
  `intersect.surface_intersector_p3` 1181 pass, 0 failed (its runner now builds the real
  caches, kept across calls, instead of tree-less default-option ones). Other callees of
  `DoIntersection` that fetch their own cache (the degenerate-curve point solve and the
  drops inside `GlobalCurveIntersect`, `LocalSurfaceIntersect`) are not given the
  intersector's cache yet; no compared case depends on it.
