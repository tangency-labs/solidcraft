Updates: WR-521

#### WR-521 (update, 2026-11-02). The tangent rule's arc walls are exact cones too (fork 3ae7bfa)

- **Location:** WR-521's tangent rule builds its walls through `sm_CreateCurvatureKeepingWalls`,
  which tangency-labs/smlib@3ae7bfa changed (see the WR-520 addendum of the same date).
- **What changes:** with `Kernel::fixes.taper_tangent_corner_planes`, an arc's wall is now
  the `SmCone` about its axis rather than a ruled B-spline, and its seams' pcurves are cut at
  the cone's spans. The fork applies the change to every arc wall regardless of the corner
  rule, and the port does the same. The tangent planes and line walls are unchanged.
- **Rust port:** no new field. The change is reached only under the existing opt-in
  `taper_tangent_corner_planes`, so the default results stay at upstream's.
  `a_half_disc_keeps_its_cone_or_gets_tangent_planes` (`issue_1449_taper_corner_rules.rs`)
  keeps its 6 faces and volume. On the tangent peg drafted 5 degrees under the tangent rule
  (`fork_9bebf34_tangent_end_cases.rs`), the bar's and the disc's cone walls are now one
  face. The front round r2 (at 9bebf34's profile) and the front and concave rounds r2
  together (at both profiles) now build within the box and pass `check_brep`, where before
  SMLib's check refused the round and the rounds together failed with status 1001. Those
  cases are marked `Here::Passes`.
