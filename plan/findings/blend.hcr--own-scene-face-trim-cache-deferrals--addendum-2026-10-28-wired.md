Updates: plan/findings/blend.hcr--own-scene-face-trim-cache-deferrals.md

# blend.hcr: face-owned GetTree wired, the three deferrals compare again (2026-10-28)

Unit: `blend.hcr` (caller), `surface.trim_srf_cache` (owner of the build).

Runtime path (confirmed with a temporary trace on `gen0103`): `ComputeEdgeVisibility`
runs with the scene's editing flag still on (upstream sets `m_bEditingEnabled = TRUE`
in `ConstructorInit` and clears it only in `ComputeGlobalVisibility`). The factory build
of each face-owned trimmed cache therefore takes `SmTrimSrfCache::BuildTree`'s
standalone branch (`m_bHaveTSurfaceCache = FALSE`, containment restored), so every later
`SmSurfaceCache::GetTree` re-runs the virtual `BuildTree`. The Rust `SurfaceCache::get_tree`
has no face to give it, which was the stub the cases hit (first in `Hcr::is_point_visible`'s
top-node check).

Fix: `TrimSrfCache::get_tree_with_face` is `GetTree` with the virtual `BuildTree`
dispatched to `build_tree_with_face`. `Hcr::is_point_visible` now builds a face-owned
cache with `smsurf_GetSurfaceCache` through the face and then makes the tracer's three
`GetTree` calls in upstream order (`AddSurfaceToGrid`: `GetTree() == NULL`,
`GetTree()->GetTopNode()`; `ExpandHierarchy`: `GetTree()`), each with the face, before
the tracer borrows the surfaces. `RayTracer::add_surface_with_fetched_cache` and
`Grid::expand_hierarchy_fetched` read that tree without calling `GetTree` again. The
bare `SurfaceCache::get_tree` stub for a face-owned trimmed cache without a face stays as is.

Result: `blend.hcr` 434 cases, 434 pass, 0 deferred, with_values 406 (before 403),
max_rel_err 0. `surface.trim_srf_cache` gains 7 direct native-face `GetTree` cases
(editing on: standalone rebuild; editing turned off between `BuildTree` and `GetTree`:
the rebuild implants the trims into the existing tree, 417 nodes on `Mouse.smb`;
trimmed tree: no rebuild; no tree: `GetTree` builds it), all passing with max_rel_err 0.
