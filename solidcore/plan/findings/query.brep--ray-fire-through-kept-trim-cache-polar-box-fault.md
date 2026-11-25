# query.brep: ray fires through the surface's kept trimmed cache fail in build_polar_box (2026-10-28)

Unit: `query.brep` (`query::brep::ray`, `face_surface_cache` / `prepare_surface_cache`,
used by the live and prepared ray services, `BrepFaceServices`).

**Upstream.** A ray fire's `GlobalLineIntersect` reaches the face's trimmed cache through
`smsurf_GetSurfaceCache` / `GetTree`: the one cache kept on the surface. A cache made
while the B-rep is being edited keeps the standalone state bits of the
`SmTrimSrfCache` constructor (`SmTrimSrfCache.cpp:186-190`). Later lookups of the face
get that same cache.

**Port.** The ray services build a private cache for each solve (`CacheObject` handle
space, `get_surface_cache_with_face`). Only the cache-tree state is recorded on the
surface's cache slot (`Surface::cache_tree_state`), not the trimmed cache itself. So a
later face query builds a fresh cache, with the flags of a build while not editing.
On main this does not change any compared case: the face_p2 and face_p3 shims drop the
load-time trimmed cache (305295d1c), and the read-time tree states already match.

**Experiment (not merged).** The solve copy was taken from the kept cache instead: the
slot's `TrimSrfCache<EntityRef>`, bound to the face, its tree built as `GetTree` builds
it, and mapped into `CacheObject` space (`query::face::p2::kept_surface_cache_of_face`,
a `get_tree` variant of `solve_surface_cache_of_face`). It regresses three units, both
in this form and in the earlier analyst adapter:
- `query.api_queries`: 106 cases fail. For example, in `face_kind4_op7` (`geometry.len`
  cpp=1, rust=0) `brep_relationship_solve` returns `InvalidInput
  { operation: "surface::surface_cache::p1::SurfaceCache::build_polar_box",
  reason: "node surface not held by the cache" }`.
- `primitive.primitive_creation_p2`: gen0028, gen0039, gen0071, gen0178 and gen0252 fail
  with `record[0]` cpp=0, rust=1007 (`SM_ERR_INVALID_INPUT`).
- `sweep.api_brep`: `planar_path_connected` fails with `counts[0]` cpp=4, rust=5.

**What is known.** In `face_kind4_op7` the cache flags are identical on the old and new
paths for every face (tree built, `have=true`, containment Done, not modified, no
boundary-curve processing, point test on, editing off). Every cache is new (none was
kept before). So the state of the cache does not cause the failure. The likely cause
is that a node's surface fails the polar-box ownership check after the copy is mapped
out of the kept cache. That could come from the copy's surface no longer being the one
its tree nodes refer to, or from a later reuse of the kept cache by the read-only
face classifications (`query::face::p2` own-cache classify,
`intersect::point_class::face_point_test_on`). This is not traced.

**Status.** Open, and not needed for any current case. Taking ray fires through the
kept cache needs that fault fixed first, then a full `--green --baseline` run.
