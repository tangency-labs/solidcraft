# blend.fillet_geom_p2: CalcRailRailInterpolation drops and lifts on the face's own surface (2026-10-31)

Updates: plan/findings/blend.fillet_geom_p6--addendum-2026-10-28-face-handle-context-wired.md

Unit: `blend.fillet_geom_p2` (`FilletBrep::calc_rail_rail_interpolation`, upstream
`SmFilletEdge::CalcRailRailInterpolation`); the cache fallback is in
`surface/trim_srf_cache.rs` (`SmTrimSrfCache::BuildTree`).

## Correction

The 2026-10-28 addendum said the `NoObject` drops in
`calc_rail_rail_interpolation_with_solvers` "would land at the same
`SurfaceCache::face` stub" if a case reached them. That is wrong. The routine copied the
face's surface with `Surface::clone` (the copy constructor), which clears the owner, so
its four calls never saw the face:

- `DropCurve` of the 3D curve (`SmFilletGeom.cpp:7007`),
- `LiftCurve` of the UV curve (`:7034`),
- the re-drop after the arc-length reparametrisation (`:7050`),
- the `LiftCurve` of the concave `NxN` cubic interpolant (`:7126`).

They ran on an ownerless, untrimmed surface cache with the system zone tolerance, where
upstream uses `pFace->GetSurface()`: the face's trimmed cache, through `GetFace()`, and
the face tolerance. The deviation was silent, not deferred. The p2 runner hid it: its
original face was a bare face record holding an ownerless surface, while the shim builds
it with `CreateFaceFromSurface`.

## Fix

- The runner builds the face as the shim does (`CreateFaceFromSurface` over the natural
  domain, then `SetTolerance(SmZoneTol3d(tol), FALSE, FALSE)`), and adds 800 `railrail`
  cases biased toward curved faces, the 3D drop and face tolerances far from the system
  one.
- `calc_rail_rail_interpolation_with_solvers` evaluates `clone_for_evaluation()` of the
  face's surface (owner and face tolerance kept). It runs all four calls through
  `BrepFaceServices` with the handle `CacheObject::Surface(face)`, as `calc_blending_rail`
  already did.
- The drop (`:7007`), the lift (`:7034`) and the re-drop (`:7050`) share one services
  object, so the face's cache the first call makes, trimmed or the plain fallback, is
  the one the later calls use. That is upstream's cache slot. `SM_USE_GLOBAL_CACHE` is
  off (`SmConfig.h:80`), so `SmCacheMgr::GetOrCreateObjectCache`
  (`SmCacheMgr.cpp:1256-1291`) hands `SmSurface::CacheMakeOrValidate` the surface's own
  slot, `GetCacheObj()`. A failed trimmed build clears the slot (`SmSurface.cpp:5933`),
  and the `SM_OC_SURFACE` fallback stores the plain cache there (`:6075`). The next
  `SM_OC_TRIMSRF` request returns that cache as it is (`:5809-5829`): it is remade only
  when `m_bFaceWasModified` is set (`SmFace::Notify(SM_NO_PRE_EDIT)`) and B-rep editing
  is off. Nothing edits the face between the three calls.
- The lift of the concave `NxN` cubic interpolant (`:7126`) gets its own services.
  `sm_CreateCubicRailRailInterpolant` runs before it and may change `target` (its
  `ComputeUVPoint` can make UV trim curves, and `RailRailIntersect` runs on the B-rep;
  `TestRailRailInterpolationCurve` only reads it), and a `BrepFaceServices` keeps its
  caches only over an unchanged B-rep. Upstream reuses the slot's cache there unless the
  face was marked modified.

With the runner change alone, four cases differ from C++: `railrail0121` (2^-62 vs 0 in
UV control points), `railrail0550` and `railrail0622` (C++ `SM_ERR`, Rust success) and
`railrail0769` (C++ success, Rust `SM_ERR`). With the face surface they agree bit for
bit.

## Second deviation exposed

The face context exposed `railrail0108` and `railrail0450`: C++ `SM_ERR` (1001), Rust
`SM_ERR_NULL_POINTER` (1006) from `LiftCurve`. Both are micro-scale (1e-6) faces with
tiny face tolerances. On the first trimmed-cache build no edge use grows the face
domain, so `BuildTree`'s `SER(sSurfaceDomain.Intersect(sFaceDomain, m_sUVDomain))`
(`SmTrimSrfCache.cpp:329`) returns `SM_ERR_INVALID_INPUT`. `CacheMakeOrValidate`
fails, `GetOrCreateObjectCache` returns `NULL`, and `smsurf_GetSurfaceCache` falls back
to the plain surface cache. The port raised that status as `SolidError::InvalidInput`
(`Extent2d::intersect`), which the cache factory passes up without falling back, so
`LiftCurve`'s `NER(pSurfaceCache)` saw no cache. `TrimSrfCache::build_tree_with_face`
now returns the status (`SolidError::Upstream { code: SM_ERR_INVALID_INPUT }`), and the
factory falls back as upstream does. The shared services then keep that plain cache for
the later calls, as upstream's slot does, and both cases end in `SM_ERR` as C++ does.

A per-call design (a new `BrepFaceServices` for each drop and lift) was also tried. It
tries the trimmed build again on every call, which upstream does not do. On these two
cases the retry finds the UV trim curves the failed first build made, gets a trimmed
cache and fails with `SM_ERR` too, and it also passed the whole default run and the
`--seeds 1000` run. So the difftest does not tell the two designs apart; the shared one
is kept because it is what upstream does.

## Evidence

Difftest, `--jobs 1`, default seeds:

- `blend.fillet_geom_p2`: 1151/1151, `max_rel_err=0`. With the runner change alone it
  was 1147/1151, and main's 351 cases were 351/351. With `--seeds 1000`, 2051/2051; the
  runner change alone failed the same four cases there.
- The `trim_srf_cache.rs` status also reaches the callers of `build_tree_with_face`
  outside the factory: `TrimSrfCache::get_tree_with_face`, `face_cache_tree`
  (`query/face/p2.rs`, the `GetTree` of a face's kept cache) and `face_context.rs`'s
  `face_surface_cache` (the Boolean face caches of
  `boolean/topology_intersector/p1.rs`), which build a tree the factory left unbuilt
  during a Boolean operation. Their callers now get a disjoint face domain as
  `SolidError::Upstream { code: 1007 }`, not `InvalidInput`, so code that singles out
  `InvalidInput` treats it as a native status: `BrepFaceServices::classify`, for
  example, ignores it under `ignore_native_status` instead of passing it up. 32 units
  that build face caches were run with the build before the library change and with this
  one, and give the same summary lines and, case by case, the same outcomes and value
  counts (36,829 cases): `blend.fillet_geom_p3` (318 of 325, 7 rejected),
  `blend.fillet_geom_p6` (400), `blend.fillet_executive_p3` (769 of 771, 2 rejected),
  `blend.fillet_corner_p1` (308), `blend.fillet_corner_p2` (606 of 630, 24 rejected),
  `blend.fillet_corner_p3` (994 of 1013, 19 rejected), `boolean.api_brep` (1094 of 1097,
  3 parked), `boolean.merge_p1`-`p5` (251, 45, 438, 311, 140),
  `boolean.topology_intersector_p1`-`p3` (845, 913, 271), `query.face_p1`-`p4` (2266,
  1401, 1199, 503), `query.api_queries` (1399), `topology.face_p1` (2532 of 2569, 37
  rejected), `topology.face_p2` (2692), `topology.face_p3` (2407), `topology.face_p4`
  (5930), `topology.brep_planar_section` (167), `surface.trim_srf_cache` (779),
  `surface.surface_cache_p1`-`p3` (872, 913, 600), `heal.face_props` (1873),
  `intersect.topology_solver_p2` (2575) and `offset.offset_geometry_creation` (910 of
  926, 16 rejected).
- `--compare-reports` of those 33 reports against the reports from before the change
  (main's for p2): 0 units regressed, every baseline case retained.

## Still open

- The `NxN`-concave lift (`:7126`) is unreached: the p2 shim's corner is
  `SM_FCR_UNKNOWN`. Its fresh cache (above) can differ from upstream's reused one, for
  example when the face's first trimmed build failed and upstream keeps the plain
  fallback.
- One `BrepFaceServices` shared over calls that leave the B-rep unchanged matches
  upstream's slot, as `calc_blending_rail` (`fillet_geom/p6.rs`) and these three calls
  do. What differs is a new services object: it starts without caches, where upstream's
  slot keeps the face surface's cache from earlier calls in the operation until the face
  is marked modified. When an earlier routine's trimmed build failed and left the plain
  fallback in the slot, the port's next routine tries the trimmed build again, and it
  can now succeed on the UV trim curves the failed build made. No case shows it yet.
- The routine evaluates a copy of the face's surface taken before the first cache
  build. A build whose `CreateUVTrimCurves` raises the face tolerance (even when it
  fails) leaves the later calls reading the old tolerance through the copy, where
  upstream's `pSurf` reads `GetFace()->GetTolerance()` live. `calc_blending_rail` has the
  same limit. No case shows it.
- `srf_srf_intersection_with_context` moves its context into the general SSI branch, so
  its drops of missing UV curves (`fillet_geom/p2.rs`, the two `surface_drop_curve`
  calls) still run `NoObject` on a face-owned surface from `fillet_geom/p5.rs`. Reach is
  unproven.
