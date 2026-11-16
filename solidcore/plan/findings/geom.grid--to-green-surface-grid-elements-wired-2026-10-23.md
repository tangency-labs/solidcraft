# geom.grid to green: surface grid elements wired (2026-10-23)

- **What was wired.** `SmSurfaceGridElement::Init` is now `SurfaceGridElement::init` (with
  `from_leaf_data`, the body of `intersect::ray_tracer::surface_leaf`, made generic in the
  handle type and moved to `geom::grid`). `Grid::expand_hierarchy(k, id, cache)` takes the
  surface's one cache (`smsurf_GetSurfaceCache`) and `Grid::add_surface_node` is
  `AddElement(SM_GE_SURFACE)` from a tree node; `ElementSource::Surface` is gone.
  `RayTracer::add_surface_to_grid` calls `Grid::expand_hierarchy`. Both dispatch gaps of the
  unit (`smsurf_GetSurfaceCache` on a bare handle, `SmBezierPatch::GetPseudoBox` from a
  node) are closed. `Init` reads the cache's existing tree and never fetches it (upstream
  `Init` takes a node; `GetTree` would rebuild the tree of a face-owned surface).
- **Harness.** The explicit-cache ops of the runner (`define_surface`, `add_cached_hier`,
  `expand_cached`, `add_cached_leaf`, `observe_cached`) now run these library functions on
  the real cache nodes of both sides (`native.rs` calls `SurfaceGridElement::init`; it used
  to copy `surface_leaf`). The two fixtures that were deferred,
  `raytracer_surface_hierarchy_expand` and `surface_leaf_element`, are written with those
  ops (a 3 x 3 span affine patch expanded, and two nodes of a 2 x 2 patch added) instead of
  opaque object labels, which cannot name a surface; opaque `add_hier { surface: true }`
  elements stay dump-only and `expand` of one is an invalid program (the shim would
  dereference the fake pointer). The legacy `add_surface` op (a node label without a
  surface) is gone from the runner. A runner test asserts that both fixtures run on both
  sides with more than one 51-value leaf record compared.
- **Numbers.** `geom.grid` 615 cases, 613 pass + 2 deferred -> with the six explicit-cache
  fixtures of #620: 621 cases, 621 pass, 0 deferred, all 621 compare values, max_rel_err 0
  (6000 extra seeds: 6021/6021). `intersect.ray_tracer` 421/421 (unchanged, its 51-value
  leaf records stay bit-identical) and `blend.hcr` 396 pass, 38 deferred (topology.edge 37,
  topology.face_p2 1), unchanged.
- **Statuses.** Upstream `Init` is `void` with `SE`, `ExpandHierarchy` ignores `AddElement`'s
  status and `AddSurfaceToGrid` ignores `ExpandHierarchy`'s; the port propagates failures
  (an `Evaluate` that fails even without `non_zero_tangents`, `Unported`).
- **A non-Bezier cache node is only reached by a unit test.** The cache trees of the
  harness's single-patch surfaces only hold `SM_AD_BEZIER_SURFACE` nodes, so the library's
  `InvalidInput` for a node without a Bezier patch (upstream casts it and reads garbage) is
  covered by the `geom::grid` unit test `add_surface_node_rejects_what_has_no_patch`, which
  makes `SM_AD_AUX_DATA` children with `subdivide_node`.
- **Surface cache size at tiny scale (why random surfaces are not generated).** A random
  bicubic Bezier patch of size ~1e-3 (net perturbation 30% of the patch size, so not flat)
  made `get_tree` grow past 418,000 tree nodes within 100 s without finishing, on the Rust
  side; the same shape at scale 1 or more finishes in milliseconds (a flat 1e-3 patch: 39
  nodes). The C++ shim runs the same program to completion in about 15 s per run, so the
  large subdivision is shared behaviour and not a Rust-vs-C++ disagreement; the Rust cache is
  at least 7 times slower on it. The threshold that keeps the subdivision going at that size
  and the speed difference were not investigated. The explicit-cache ops accept only the
  bounded affine family (#620), which avoids the problem.
