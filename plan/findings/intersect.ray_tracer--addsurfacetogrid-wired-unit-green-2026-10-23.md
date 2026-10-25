# intersect.ray_tracer: AddSurfaceToGrid wired, unit green (2026-10-23)

`RayTracer::add_surface_to_grid` was an `Unported(smsurf_GetSurfaceCache)` stub (6 deferred
cases) because `Grid::expand_hierarchy` holds a bare surface handle and cannot reach the
`Surface`. It now builds the cache itself and expands with `Grid::expand_hierarchy_with`,
reading each leaf's `SmBezierPatch` data, pseudo box and polar box from the cache.

- **Trap found by the new comparison.** The surface cache must be built with the surface's
  tracer index as its handle (`surface_cache(k, surface, Some(index))`). With `None`, every
  node's `mBA_pSurface` is `None`, so `SurfaceCache::compute_polar_box` fails with the
  port-only `InvalidInput("node has no surface")` (upstream would dereference a null
  pointer), `SurfaceCache::polar_box` ignores every failure but `Unported` and returns an
  *uninitialised* polar box. `polar_box`/`polar_box_unstored` should propagate that
  `InvalidInput` (only a port bug produces it); not changed here, other units call them.
  Positions, boxes and mid-point data were still bit-identical to the C++, and most
  ray walks too, so only the shim's new per-leaf record (pseudo box and polar box state,
  domain and basis) showed it. Any other caller that builds a cache with no handle and then
  reads polar boxes has the same silent failure.
- **Still a dispatch gap:** `Grid::expand_hierarchy` on a bare handle (`geom/grid.rs`), which
  is what `geom.grid`'s own difftest `Op::Expand` calls (2 deferred cases of `geom.grid`,
  `surface.surface_p2` 1 and `surface.surface_cache_p1` 1). It needs a store that turns a
  handle into a `Surface`; `intersect::ray_tracer` does not need it any more.
- **Second caller:** `blend.hcr` adds its ray surfaces through `add_surface_to_grid`
  (`blend/hcr.rs`). difftest `blend.hcr`: 386 pass / 48 deferred (`surface.surface_p2` 10)
  -> 396 pass / 38 deferred, 0 failed, max_rel_err 0, the 10 now compared with the C++.
  The expansion ignores trims (`surface_cache` is the untrimmed cache; a surface that has a
  face is `Unported` from `get_tree`), so a `Face` surface whose owner is not set on the
  `Surface` would get leaves outside its trim loops that upstream skips; no case shows it.
- Known gaps in the `add_surface` check: the hierarchy element and the grid-element boxes
  and voxel lists are not in the record (ray walks reach them only through distances);
  a crafted near-degenerate Bezier net can make the C++ cache build subdivide without
  bound (Rust stops at `MAX_SUBDIVISION_NODES`), which a `--fuzz` run could hit.
- difftest `intersect.ray_tracer`: 421 cases, 421 pass, 0 deferred, max_rel_err 0, 6000 extra
  generated seeds clean; the record now compares `AddSurfaceToGrid`'s status and leaves.
  The `chance(0.2)` draw in the generator shifts the random stream, so the generated cases
  after it are different cases than before (same count).
