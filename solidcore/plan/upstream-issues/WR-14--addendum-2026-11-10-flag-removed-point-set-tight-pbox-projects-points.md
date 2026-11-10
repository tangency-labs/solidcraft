Updates: WR-14

#### WR-14 (update, 2026-11-10). `point_set_tight_pbox_projects_points` is removed; the unprojected points stay

`PointSet3d::tight_pbox` (`SmPointSet3d::SetUpTightPseudoBox`) measures hull-chord angles on the unprojected points, as upstream does. The projections upstream discards change nothing beyond roundoff, because `ccw_angle_between` projects both vectors onto the box plane itself. The flag is gone, along with its projected path. Removed: the field, `PointSet3d::tight_pbox_with_fixes`, `tight_pbox_with_kernel` and `set_tight_pbox_projection_fix`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden row changes. A build with the projection differs from one without it by at most 8.3e-14 in 66 raw `geom.point_set` values, all within the golden tolerance. Roundoff can still pick the other of two candidate rotations at an exact tie. The unit test that crafted such a tie, with points 4e5 off the box plane, now checks only that every box holds its points.
- **Tests:** `geom::point_set` (`wr14_tight_box_on_unprojected_points`).
