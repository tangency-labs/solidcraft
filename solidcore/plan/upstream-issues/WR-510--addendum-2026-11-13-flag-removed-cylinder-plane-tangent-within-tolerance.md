Updates: WR-510

#### WR-510 (update, 2026-11-13). `cylinder_plane_tangent_within_tolerance` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A13a, which retires three surface flags). `Cone::intersect_with_plane_owned` (`surface/cone/p3.rs`) takes a plane parallel to a cylinder's axis as tangent also when it lies within the larger zone tolerance of the cone's and the plane's face; bare surfaces (zone tolerance 0) keep upstream's test.

- **Tests:** `issue1525_plane_within_tolerance_of_wall_is_tangent` runs on `Kernel::default()`; its flag-off half is gone. `docs/ssi.md` lists the case as fixed.
- **Results:** no goldens row moves (all 283 units, measured at the retiring change on the default kernel). The batch survey's group A holds for this flag.
