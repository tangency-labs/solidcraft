# blend.fillet_executive_p3: rollover-404, the fast-drop cache read is kept without the slot's tree state (2026-11-20)

Updates: plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin--addendum-2026-10-27-fast-drop-cache-state.md

- **The read is kept.** The fast-drop face test still reads the face cache as `smsurf_GetSurfaceCache` hands it out without `GetTree`. During a Boolean operation, its tree is deferred and point testing is on. The method is renamed from `surface_cache_state` to `SurfaceSolveContext::surface_cache_without_get_tree`. It keeps all its implementations: `BrepFaceServices`, `SidedFaceServices`, `CurveFaceServices`, `LiveSurfaceContext`, the topology intersector's `FaceCacheContext` and `OffsetSurfaceSolveContext`. A solve's `GetTree` still builds the kept cache in place.
- **What the fast-drop readers see changes in two situations.** Both come from removing the slot's tree state. All four reader paths read the point-test bit:
  - the context read in `GlobalPointSolve` (`surface/surface/p4.rs`);
  - `live_face_fast_drop_test` (`intersect/point_class.rs`);
  - `face_point_test_on` (the kept cache, `intersect/point_class.rs`);
  - the vertex-use solve (`topology/vertexuse.rs`).
- **Situation A: during a Boolean operation, the slot said "built".**
  - Main forced a tree build there, because an earlier cache of the surface, since discarded, had been built. The reader now gets the constructor's bits of a cache whose tree is deferred.
  - Counted on main with a temporary counter (forced builds, and how many of those built caches had point testing off):

    | Unit | Forced builds | Point testing off |
    |---|---|---|
    | `blend.fillet_executive_p3` | 1007 | 30 |
    | `boolean.api_brep` | 103 | 2 |
    | `offset.offset_executive_p2` | 3915 | 0 |
    | 41 other units, including the boolean, query, topology and surface units | 0 | 0 |

  - With the branch removed, `blend.fillet_executive_p3` (798 cases, its known failures unchanged, rollover-404 among them) and `boolean.api_brep` pass unchanged. No golden sees the 32 differing bits.
- **Situation B: outside a Boolean operation, the slot said "unbuilt".** That means a cache of the surface had been requested during an earlier Boolean operation and never built.
  - Main made the new cache without a tree, so the reader saw the constructor's bits: point testing on. Now the cache is built when it is made.
  - On a face with loop uses, the build leaves the bit on, so readers see the same bit.
  - On a face without loop uses, or a face being edited standalone, or with no face, the build turns it off. That is where the bit now differs.
  - Counted on main with a temporary counter. The columns are:
    - **caches made in situation B;**
    - **reads:** fast-drop reads of the bit on such a cache, still without a tree, outside a Boolean operation, per reader path (context / live / kept / vertex use);
    - **differ:** how many of those reads were on a face where the build turns the bit off.

    | Run | Caches made | Reads | Differ |
    |---|---|---|---|
    | `blend.fillet_executive_p3` | 88 | 11 / 0 / 0 / 0 | 0 |
    | `boolean.api_brep` | 27 | 74 / 0 / 0 / 0 | 0 |
    | kernel cases (`solidcore-ops`) | 63 | 348 / 0 / 0 / 0 | 0 |
    | 15 units with caches made but no reads: `topology.brep_planar_section` 131, `primitive.api_trim_ops` 72, `topology.brep_p3` 40, `primitive.primitive_creation_p2` 36, `topology.face_p3` 34, `surface.surface_p1` 27, `offset.offset_executive_p2` 16, `boolean.topology_intersector_p2` 14, `offset.offset_geometry_creation` 6, `topology.brep_p1` 5, `topology.brep_p2` 2, `sweep.api_brep` 2, `boolean.topology_intersector_p1` 1, `query.face_p2` 1, `topology.face_p2` 1 | 388 | 0 | 0 |
    | 70 other units | 0 | 0 | 0 |

  - So situation B changes no bit in any of the 87 units or the kernel cases.
- **Why that set covers situation B.**
  - Main set a slot to "unbuilt" only in `trim_srf_cache_tree`, when a cache was requested while `doing_boolean` was set, or copied a slot's state in `copy_cache_slot_of` (the renumbered copy in `topology.rs`).
  - `doing_boolean` is set only in these places:
    - heal: stitch;
    - topology: `face/p1`, `brep/cutting`, `brep/p7` (the read), `brep/p9`, `edge`;
    - blend: the fillet executive, its session, and the solver's rails;
    - offset: the offset executive;
    - primitive: the sweeps and skin of primitive creation;
    - boolean: the merge and the topology intersector;
    - sweep: the topology sweep.
  - The 87 units are the goldens units of those modules, of the readers' modules (`intersect`, `query`, `surface`) and of heal. The kernel cases run the app-level operations end to end.
- **Still dependent on history.** On a face without loops the bit depends on whether the face's cache was built: the constructor sets it on, and the tree build turns it off. `query::brep::ray::tests::fast_drop_point_test_on_a_face_without_loops_while_the_tree_is_deferred` pins both: on during a Boolean operation (tree deferred), off outside one (tree built). The context and live readers agree on both. Which value is right as a function of the face alone is a follow-up question.
