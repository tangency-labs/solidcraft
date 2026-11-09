# surface.surface_p1 / surface.surface_p11: surface cache history modelled (issue #568, follows "surface cache history" above)

- Where the C++ history came from (instrumented C++ reference, backtrace at cache creation):
  in `offset.offset_geometry_creation` gen0266 every face surface of `Mouse.smb` /
  `Endcap.smb` got its `SmTrimSrfCache` during `SmBrep::ReadFromFile` ->
  `MakeTopologyFromData` (doing a Boolean operation) -> `HealBrep` -> `SmVertexProps` ->
  `SmVertexuse::ComputeUVPoint` -> `smsurf_GetSurfaceCache`, so without a tree; nothing in
  the script called `GetTree` on them afterwards (no `BuildTreeBase` at all in the case), so
  `LiftCurve`'s `GetDomainWithContinuity` saw empty continuity arrays. In gen0129, gen0152 and
  gen0258 the lifted surface had no cache before `LiftCurve`, which made one outside a
  Boolean operation, with its tree: both sides error (`SM_ERR`).
- Model: `Surface` carries a cache slot (`CacheTreeState`: no cache / cache without tree /
  cache with tree) shared by the surface and its evaluation copies (`clone_for_evaluation`,
  standing for upstream's pointers to the one object); `Clone` (the copy constructor) starts
  an empty slot. `make_trim_srf_cache*` honour and record it (a reused tree-less cache stays
  without tree outside a Boolean operation; a built one only restores its continuity arrays
  during one), `BuildTreeBase` marks it built, `ComputeUVPoint` records the
  `smsurf_GetSurfaceCache` its global fallback makes, and the continuity readers without
  `GetTree` (`GetDomainWithContinuity`, `LiftCurve`) take their cache from
  `continuity_surface_cache` (the caller's context only when upstream's cache has a tree).
  The callers that do `GetTree` build a missing tree (`solve_cache_of`, the offset solve
  contexts, the tracer constructor's `GetTree` in `tracer_surface_cache` and
  `CreateSilhouetteCurves`).
- Difftest: the `offset.offset_geometry_creation` export appends each face surface's cache
  state after the C++ build and the runner seeds the imported surfaces with it; the
  `upstream:cache-history` deferral and `OP_LIFT_OFF_DOMAIN` are gone and all 4 cases pass.
  `surface.surface_p1` `DomCont` gained a `history` (cache made during a Boolean operation;
  then `GetTree`; or reused during one) and 96 `domhistNNNN` cases, half off the domain;
  15 of them fail without the model.
- Not modelled: upstream also recreates a trimmed cache whose face was modified once B-rep
  editing is off, and deletes caches on `SM_NO_PRE_EDIT`; the port keeps the slot across
  those (it only affects the continuity arrays of a cache made during a Boolean operation).
  Surfaces the port copies with `Clone` where upstream keeps the pointer start with an empty
  slot, as before. Other runners that import C++ B-reps read from files start every face
  surface without cache.
