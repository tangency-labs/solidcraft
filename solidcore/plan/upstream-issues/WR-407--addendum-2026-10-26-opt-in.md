#### Updates: WR-407 (2026-10-26). Both halves are opt-in; the default is pinned SMLib

- The two halves #1069 carried unconditionally are now opt-in `Kernel::fixes` fields
  (`UpstreamFixes`), off by default, so a default kernel behaves as the pinned `c9c5979`:
  - `sweep_frame_on_planar_path_normal`: `prepare_curve_sweep`
    (`crates/solidcore/src/primitive/primitive_creation/p3/geometry.rs`) starts a planar
    path's frame with its z on the plane's normal. Result-changing: it changes the swept
    geometry of every planar path whose `MakeUnitOrthoVectors` frame differs (the #434
    half circle's prepared path leaves z = 0 by more than 1 without it), and the #434 join
    goes from an error to a valid solid. No difftest case reaches a changed frame, which is
    why `primitive.primitive_creation_p3` stayed green with it on.
  - `cover_refuses_folded_continuation`: `Surface::cover_coincident_surface`
    (`crates/solidcore/src/surface/surface/p3.rs`) refuses a cover onto a folding
    continuation. Result-changing: seven generated `surface.surface_p3` cases differed
    from C++ with it on.
- **Tests:** `issue_434_planar_path_frame_keeps_the_plane_normal_as_the_frame_z` and
  `issue_434_cover_refuses_a_continuation_that_folds` (fix on; each fails with its gate
  forced off), `issue_434_default_keeps_the_upstream_sweep_frame` and
  `issue_434_default_covers_across_the_fold` (default pin). The SolidCraft fixture
  `fixtures/solidcraft/434-two-overlapping-swept-arc-walls.json` turns both fields on
  (`fixes`).
- Callers that want the fork's behaviour (SolidCraft) set both fields.
