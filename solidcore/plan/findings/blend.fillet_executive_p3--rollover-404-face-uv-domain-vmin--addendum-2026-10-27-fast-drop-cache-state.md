# blend.fillet_executive_p3: rollover-404 root cause, the fast-drop face test read a built cache (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin.md

- **Where face 11's v-min is set:** `boolean_post_process` (`boolean/merge/p1.rs`) stores the merged face domain from `SmEdge::IsTopological`.
  - That domain comes from `sm_CoverOtherCoincidentFace`, i.e. `SmSurface::CoverOtherCoincidentSurface`.
  - Its covered STEP domain comes from `CoverCoincidentSurface`.
  - For two planes that is the bilinear exit: the four corner `STEPInversion`s and nothing else.
- **First divergence:** cover 2 of the five merged covers. Its inputs are bit-identical on both sides (cover plane frame, covered plane frame and domain, test points), but its corner uvs differ.
  - Rust returns the plain `SmPlane::DropPointFast` projection, e.g. `(-1.7763568394002509e-15, 1.97e-31)`.
  - C++ returns a `GlobalPointSolve_1` (Newton) result, e.g. `(-1.3322676295501878e-15, 4.44e-16)`.
  - Covers 3 to 5 inherit the drift through the stored face domains. In cover 5, C++ lands exactly on `-2` and Rust gets `-2/0.99999999999999978 = -2.0000000000000004`; the next corner gives `-2.0000000000000013`.
- **Mechanism:** `SmSurface::GlobalPointSolve` keeps a `DropPointFast` candidate only if the face classifies it (`SmSurface.cpp:20060`-`20107`). This test runs only when the face's trimmed cache has `m_bPointTestEnabled` set.
  - In C++ the cache is made during a Boolean operation (`m_bDoingBoolean` 1), so `CacheMakeOrValidate` defers `BuildTree` (`SmSurface.cpp:5926`). The constructor's `m_bPointTestEnabled = TRUE` stands.
  - `SmFace::PointClassify` then returns `SM_PC_UNKNOWN` for the corner drops (its face UV-domain early-out), so C++ falls through to `GlobalPointSolve_1`.
  - In Rust, `prepare_surface_cache` (`query/brep/ray.rs`) built the tree eagerly. On the merged face, which has no loopuses at that point, the build ran `set_standalone_flags` (`point_test_enabled = false`). The test was skipped and the projection kept.
- **Evidence:** gdb on both sides at `c2e50ca7a`, scripts groll6 to groll9 (fixer-8). C++ shows the old cache reused, `doing_boolean 1`, no `SmTrimSrfCache::BuildTree` under the cover, and `PointClassify -> SM_PC_UNKNOWN` then `GlobalPointSolve_1` on every corner. Rust shows `prepare_surface_cache: eager build_tree_with_face ... doing_boolean true`, `set_standalone_flags`, and `point_test_enabled false` in every merged cover.
- **Fix (this PR):** `SurfaceSolveContext::surface_cache_state` returns the face cache as `smsurf_GetSurfaceCache` hands it to a reader that does not call `GetTree`. The fast-drop face test in `global_point_solve_in` reads it.
  - `BrepFaceServices` (with `SidedFaceServices` and `LiveSurfaceContext`) makes and keeps it without the tree the factory defers (`face_surface_cache(.., get_tree: false)`). A surface whose cache slot says upstream built the tree is built.
  - A solve's `build_surface_cache` (its `GetTree`) builds the kept cache in place, so later readers see the built state.
  - Other contexts keep the default (the solve's cache).
- **Follow-up:** the other face-aware solve contexts (`boolean/topology_intersector` `IntersectorBrep`, `offset` `OffsetSurfaceSolveContext`, `intersect/surface_intersector` `TrimmedFaceContext`) still answer the built cache for the fast-drop test. Check them against upstream's deferred-tree state when a case reaches them.
