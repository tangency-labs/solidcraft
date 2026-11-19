Updates: WR-58

#### WR-58 (update, 2026-11-18). `point_class_gap_3d_propagates_no_drop_error` is removed; without a drop point the stored gap answers

The flag is gone. `PointClassification::gap_3d` (`intersect/point_class.rs`) with a surface always
passes failing statuses of the object point (`FindObjPoint3d`), the two surface drops
(`GlobalPointSolve`) and the surface evaluations on, where upstream ignores them and measures from
an undefined point. Removed: the flag's file `kernel/fixes/point_class_gap_3d_propagates_no_drop_error.md` (the
field `UpstreamFixes::point_class_gap_3d_propagates_no_drop_error`), its off path, the helper
`ignored_or_pass_on` (this was its only user) and the test settings. SolidCraft's profile turns on
every flag it doesn't hold back, so it needs no edit; `issue_2260`'s frozen list skips the name.

When the object point has no single drop point on the surface (or on its face, within the face's
trim, when the surface belongs to a face that tests its drops), even at a hundred times its zone
tolerance, the answer is now the classification's stored gap, the measure `GetGap3d(NULL)` gives,
not an error. The in-surface measure is the component tangent to the surface of the offset from
the object point's drop point to the object point; without a drop point it is not defined. Upstream
returns `SM_ERR` there as the gap value, which `SmCurveClassification::HasNearMisses` reads as a gap
of 1001; the opt-in error made `has_near_misses(in_surface_gaps = true)` refuse instead.

A B-spline surface without a NURB is one such case: its drop returns no solution, not an error.
The 2026-11-01 addendum listed it as a failing surface solve; the error its test saw was the opt-in
error for no drop point.

- **Reach:** only the goldens call `has_near_misses` with `in_surface_gaps = true`; every library
  caller (`topology/face/p1.rs`, `topology/face/p4.rs`) passes `false`, as upstream's
  `SmFace` callers use the default. No saved design or rig case reaches the changed branch.
- **Tests:** `wr58_gap_3d_without_a_drop_point_is_the_stored_gap`,
  `wr58_gap_3d_passes_on_object_point_errors` and
  `wr58_gap_3d_within_drop_range_and_without_surface` in `intersect/point_class.rs`, on the default
  kernel.
- **Results:** `intersect.curve_class_p7` is unchanged against main (1673 cases); with the flag on,
  31 of them were refused (1001). `intersect.point_class` moves 61 rows, all through `gap_3d` with a
  surface: a gap of 1001.0 becomes the stored gap, and a generated script whose gap now fails with a
  passed-on status is rejected by the generator, which draws the next one. A substitution replay
  (the fix with the old gap value put back at every `Gap` op reproduces main's rows) shows that
  `gap_3d` is the only cause.
