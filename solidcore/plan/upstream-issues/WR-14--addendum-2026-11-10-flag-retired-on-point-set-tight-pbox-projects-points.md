Updates: WR-14

#### WR-14 (update, 2026-11-10). `point_set_tight_pbox_projects_points` is removed; the projection is the only path

`PointSet3d::tight_pbox` (upstream `SmPointSet3d::SetUpTightPseudoBox`) projects the four boundary points and each candidate point onto the plane through the pseudo box's centre before it forms the chord vectors for the hull-edge angles. Upstream discards those projections. The projection is now the only path, and the flag is gone. Removed: the field, `PointSet3d::tight_pbox_with_fixes`, `tight_pbox_with_kernel` and `set_tight_pbox_projection_fix`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

`ccw_angle_between` projects both vectors onto the plane itself, so the two paths agree in exact arithmetic. For points far off the plane, however, the unprojected chords carry roundoff that can decide an exact tie between two candidate rotations. In the crafted case (points up to 4e5 off the plane, with the hull's long edge and a chamfer both 0.08 rad from the axis), the unprojected path picks the chamfer. Its rotated box is then rejected, leaving an area above 75. The projected path picks the long edge: area below 53 and volume below 4.2e7, about 30% smaller. That box holds every point.

- **Evidence** (all 1378 `geom.point_set` tight-box values, projected against unprojected): none is larger. 1295 are identical and 83 differ at roundoff only. Every changed box holds its points. No golden row changes in any unit (all 283 pass).
- **Tests:** `geom::point_set` (`wr14_tight_box_on_projected_points`, which asserts the crafted case's rotation, area and volume).
