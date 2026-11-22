# geom.grid (partial; no disagreements)

615 default cases (15 fixture, 600 generated) — 613 pass bit for bit, 2 deferred
(`SmSurfaceGridElement::Init` -> surface.surface_cache_p1, `SmSurfaceHierarchyElement::
ExpandHierarchy` -> surface.surface_p2); 30k-seed run clean. Upstream behaviour worth knowing:

- **`Step3DDDA` returns the last voxel's hits together with `rbExitedGrid = TRUE`.** The
  exit test runs before the "found something" break, so callers must use the element lists
  even when the ray left the grid. Stepping again after an exit indexes voxel `m_lOut*`
  (out of bounds); the port returns `InvalidInput`, the shim never does it.
- **`FindVoxelsInBox` builds a tolerance-expanded copy of the box and never uses it**: the
  voxel range comes from the unexpanded box (so `AddElement` / `AddHierarchyElement` do
  not pad by `m_dVoxelTolerance`). Kept.
- `GetVoxelAddress` on a grid with a zero-size axis converts NaN to `ULONG` (UB); the port's
  saturating cast gives 0 and the shim only accepts grids with positive sizes. A zero voxel
  count makes `~SmGrid` read `m_aVoxels[0][0]` out of bounds; `Grid::new` rejects it.
- `Setup3DDDA` writes `m_lAddress` before its "test point outside the grid" early return,
  and leaves `m_lStep*`/`m_lOut*` unset for axes the ray does not move along (never read).
- `AddElement(..., SM_GE_HIERARCHY, ...)` dereferences a NULL element; not expressible in
  the port (`ElementSource` has no such variant).
- No `UNPORTED(geom.grid)` stubs existed to wire.
