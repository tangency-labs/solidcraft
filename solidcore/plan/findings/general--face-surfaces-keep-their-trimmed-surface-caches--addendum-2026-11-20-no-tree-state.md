# Face surfaces keep their trimmed surface caches: the slot no longer records a tree state (2026-11-20)

Updates: plan/findings/general--face-surfaces-keep-their-trimmed-surface-caches.md

- **The slot.** It holds only the face surface's trimmed cache (`TrimCacheSlot`: empty, stored or checked out). `Surface::cache_tree_state`, its setter and `CacheTreeState` are removed.
- **Making a new cache.** Whether a new cache is built depends only on the kernel context: it is deferred during a Boolean operation and built otherwise. It no longer depends on whether a cache of the surface was made before.
- **The continuity readers.** `GetDomainWithContinuity`, `LiftCurve` and the drop's boundary breaks read the surface's own continuities (`Surface::continuities`, a memo on the surface), so they need no cache. See `plan/findings/surface.surface_p1--surface-cache-history-modelled-issue-568-follows-surface--addendum-2026-11-20-history-removed.md`.
- **Unchanged.**
  - The kept cache and its reuse.
  - The standalone cache of a surface whose owner is cleared.
  - The fast-drop face test's reader of a cache without `GetTree` (`SurfaceSolveContext::surface_cache_without_get_tree`).
- **The harness.** The topology.brep_p4 import no longer seeds cache states. It already did not since 2026-10-30.
