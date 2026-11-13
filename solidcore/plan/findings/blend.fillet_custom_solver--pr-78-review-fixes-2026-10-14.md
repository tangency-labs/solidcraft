# blend.fillet_custom_solver (PR #78 review fixes, 2026-10-14)

- **Corpus.** 448 cases (48 fixture, 400 generated): 359 pass at max_rel_err 0, 89
  deferred (surface.offset_surface). The earlier note's "17 fixture, 405 generated"
  split was wrong (it was 22 + 400). 26 review fixtures (`adv_*`) cover antiparallel,
  identical and nearly parallel line rails, zero radius, `P2 == P1`, collinear
  points, rational / double-knot / polyline / quadratic rails, tolerance equal to the
  rail gap, `r2 > r1`, angles past pi, arc + line, non-coaxial and reversed arcs,
  multi-span arcs and tolerance 0.
- **FFI guard for `CreateSurface` intervals.** Rails over different natural
  intervals are undefined in C++ only in the skinning branch (rail synchronisation
  reads `pIntersectionsWithRails` past `k`). The guard now also admits two
  non-rational 2-point degree-1 rails at least `1e-7` long whose directions agree to
  within `5e-7` rad (half the `SM_EFF_ZERO_SQRT` sweep test, so they always take the
  ruled branch); `EvaluatePoint` clamps rail 2's off-domain parameters. The
  surface-of-revolution branch (both rails arcs) is also valid with different
  intervals but stays guarded, because predicting `IsArc` in the guard is not cheap.
- **Not compared yet.** The C++ shim clears the solvers' edge uses, so the
  edge-use / `GlobalPointSolve` / `IsClosed` / `DropPoint` branch of both
  `LoadInitialValues` is never run on either side (Rust reaches it only after the
  Unported offset `EvaluatePoint`). A static review found it matches upstream
  (`IsClosed` tol 0.0, `Evaluate`'s `bNonZeroTangents = TRUE`, the face/surface
  identity test, `EFF_ZERO_SQ` for the base `GlobalPointSolve`, the pre-extension
  domain for `LocalPointSolve`). Compare it once surface.offset_surface lands.
- **Wiring note.** `offset_compute_inward_vector` is tagged
  `UNPORTED(surface.offset_surface)` (`SmOffsetSurface::Evaluate`). Wire it
  through the ported `surface::surface::p7::compute_inward_vector`, not a new copy.
