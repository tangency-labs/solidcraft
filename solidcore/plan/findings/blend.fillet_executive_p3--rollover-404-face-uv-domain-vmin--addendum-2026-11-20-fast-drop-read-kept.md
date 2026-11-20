# blend.fillet_executive_p3: rollover-404, the fast-drop cache read is kept without the slot's tree state (2026-11-20)

Updates: plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin--addendum-2026-10-27-fast-drop-cache-state.md

- **The read is kept.** The fast-drop face test still reads the face cache as `smsurf_GetSurfaceCache` hands it out without `GetTree`. During a Boolean operation, its tree is deferred and point testing is on. The method is renamed from `surface_cache_state` to `SurfaceSolveContext::surface_cache_without_get_tree`. It keeps all its implementations: `BrepFaceServices`, `SidedFaceServices`, `CurveFaceServices`, `LiveSurfaceContext`, the topology intersector's `FaceCacheContext` and `OffsetSurfaceSolveContext`. A solve's `GetTree` still builds the kept cache in place.
- **Removed: "A surface whose cache slot says upstream built the tree is built."**
  - That branch answered "built" because an earlier cache of the surface, since discarded, had been built. The slot's tree state is gone, so the reader now sees the cache it actually gets.
  - Counted on main with a temporary counter (reads where the branch forced a build during a Boolean operation, and how many of those built caches had point testing off):

    | Unit | Forced builds | Point testing off |
    |---|---|---|
    | `blend.fillet_executive_p3` | 1007 | 30 |
    | `boolean.api_brep` | 103 | 2 |
    | `offset.offset_executive_p2` | 3915 | 0 |
    | 41 other units, including the boolean, query, topology and surface units | 0 | 0 |

  - With the branch removed, `blend.fillet_executive_p3` (798 cases, its known failures unchanged, rollover-404 among them) and `boolean.api_brep` pass unchanged. No golden sees the 32 differing bits.
- **Still dependent on history.** On a face without loops the bit depends on whether the face's cache was built: the constructor sets it on, and the tree build turns it off. Which value is right as a function of the face alone is a follow-up question.
