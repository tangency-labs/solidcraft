Updates: WR-32

#### WR-32 addendum (2026-10-31): opt-in antiparallel normal offset for circles

`Kernel::fixes.circle_offset_antiparallel_normal` enables distinguishing antiparallel
offset plane normals from parallel ones in `SmCircle::CreateSimpleOffset`
(`Ellipse::create_simple_offset`). Upstream's `IsParallelTo(+Z)` accepted antiparallel
normals, causing the `-Z` branch (radius minus offset distance) to be dead and growing
the circle instead of shrinking it. It is false by default, preserving pinned C++
v1.0.1 bit-parity across all callers and difftests.

When enabled, antiparallel offset plane normals evaluate `radius - offset_distance`
via `offset_plane_normal.dot(my_z_axis) < 0.0`, properly shrinking positive radius,
degenerating to a point B-spline curve within `approx_tol_3d`, or returning an error
when negative beyond tolerance.

- **Code:** `crates/solidcore/src/kernel.rs` (`circle_offset_antiparallel_normal`),
  `crates/solidcore/src/curve/ellipse/p1.rs` (`Ellipse::create_simple_offset`).
- **Tests:** `crates/solidcore/src/curve/ellipse/p1.rs::tests::wr32_*`
  (`wr32_circle_offset_antiparallel_default_preserves_expansion`,
  `wr32_circle_offset_antiparallel_opt_in_shrinks_positive_radius`,
  `wr32_circle_offset_antiparallel_opt_in_degenerate_radius`,
  `wr32_circle_offset_antiparallel_opt_in_negative_radius_error`,
  `wr32_circle_offset_parallel_normal_invariance`).
