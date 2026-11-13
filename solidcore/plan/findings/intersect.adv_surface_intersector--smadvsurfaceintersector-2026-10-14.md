# intersect.adv_surface_intersector (`SmAdvSurfaceIntersector`, 2026-10-14)

- **Lazy polar-box build not compared.** The shim's tree reader rejects patch/head nodes
  without polar-box vectors, so the lazy-build path of `AdvSurfaceIntersector::polar_box`
  (`cache.clone().polar_box(id)`) never runs against the C++. It only matters through
  `BranchIsGaussSeparate`, which upstream `SolveBranch` never reaches (WR-70). Once
  `surface.surface_cache_p3` can build real trees, add a direct `OP_GAUSS` case on a real
  cache.
- **`FindInteriorCurves` after `LocalSurfaceIntersect` not compared.** `IsPointOnCurve`,
  `ComputePointValues`, `TraceCurve` and `FlushCurve` sit behind unported units
  (`surface.surface_p11`, `intersect.surface_intersector_p2`, `_p4`). The port propagates
  `Unported` from `ComputePointValues` where upstream `continue`s on failure; other errors
  still `continue`. When those units land, re-run the 16 deferred cases and re-check that
  branch.
- **Review fixtures.** `fixtures/intersect.adv_surface_intersector/review_adversarial_singular.json`
  holds 398 reviewer singular-point programs: saddle/saddle, bowl/bowl and saddle/cylinder
  Dupin intersections; curvatures at `EFF_ZERO_SQRT` and at a 1000x ratio; principal frames
  swapped or rotated by tiny angles; opposite normals; trace direction -1; start points on
  domain corners; the point itself or another point as the last curve node; zero previous
  derivatives; 1e-3 and 1e3 scales. The reviewer's generator made 400; the 398 kept are the
  ones it accepted as defined in the C++. All 398 pass bit-identical.
