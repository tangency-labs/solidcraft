# surface.surface_p1 / surface.surface_p11: surface cache history no longer modelled (addendum to "surface cache history modelled")

The cache-history model is removed. A surface's continuity arrays are now a pure function of
the surface, so `GetDomainWithContinuity` and `LiftCurve` give the same answer whatever ran
before. This is a deliberate deviation from upstream, whose answer off the natural domain
depends on whether an earlier call happened to build that surface's cache tree.

- **Model as built.**
  - `Surface::continuities` (`surface/surface/p6.rs`) computes the U and V arrays once and
    keeps them in a memo beside the singularities and boundary memos. The memo is keyed like
    `CurveMemo` and shared by exact copies (`clone_for_evaluation`, `clone_exact_bounded`).
    `Clone` and every edit start it empty.
  - `BuildTreeBase` takes its arrays from that memo.
  - A cache whose tree is still deferred (made during a Boolean operation) gets its arrays
    from `SurfaceCache::fill_continuities` when a continuity reader asks for them
    (`continuity_surface_cache`, `boundary_start_points`). The tree itself stays deferred.
- **Removed.** The surface's remembered tree state (`CacheTreeState`, its slot field and
  accessors, `note_surface_cache`, `cache_tree_built_on_get`) and the solve contexts' reader
  of it (`SurfaceSolveContext::surface_cache_state`). Readers that need a tree now build it.
- **`surface.surface_p1` `DomCont`.**
  - The `history` input is gone. The 96 `domhistNNNN` cases stay as continuity-domain cases
    with points off the domain (`off_domain`). That is still the only place the end-knot
    clamp is pinned.
  - The generator keeps the draw that picked a history, so every case's inputs are
    unchanged.
  - 15 outputs changed: 0009, 0011, 0023, 0043, 0049, 0061, 0062, 0063, 0067, 0070, 0071,
    0073, 0078, 0079, 0083.
  - Proof: each new output is bit-identical to the output of the kernel before this change
    with no history (a fresh cache, its arrays filled). The old outputs came from the
    empty arrays of a cache without tree. The new kernel run with the old history harness
    also produces the identical expected file.
- **`offset.offset_geometry_creation` gen0266.**
  - `LiftCurve`'s only segment starts at uv `(18.834, 34.638)` with direction
    `(0.6726, -0.7357)` over `[-43.708, 49.763]`. The natural domain is `[-0.05, 1.05]²`.
  - That line meets u ∈ [-0.05, 1.05] only for t ∈ [-28.07, -26.43], where v ≈ 55. It meets
    v ∈ [-0.05, 1.05] only for t ∈ [45.64, 47.14], where u ≈ 49.5. So no point of the
    segment lies in the domain.
  - The old run lifted it anyway, from the whole domain the empty arrays allowed. Its result
    deviates from the surface by 1.847e10 against a tolerance of 3.3e-6, measured with an
    uncommitted probe as the largest distance from `S(uv(t))` to the lifted curve over 2001
    values of t.
  - The C1 domain around the start point is now the corner `[1.05, 1.05]`. `LiftCurve`
    refuses, which is correct, and the case records the refusal.
- **Not changed.** These readers of the point-test bit of a cache without tree still read it
  without building:
  - `intersect/point_class.rs`, the live fast-drop test and `face_point_test_on`;
  - `topology/vertexuse.rs`, `global_point_solve`.

  Building there can recurse through the leased-cache path. The bit differs from a built
  cache's only on a face without loops.
