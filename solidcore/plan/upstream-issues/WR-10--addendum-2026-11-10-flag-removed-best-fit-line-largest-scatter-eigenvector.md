Updates: WR-10

#### WR-10 (update, 2026-11-10). `best_fit_line_largest_scatter_eigenvector` is removed

`best_fit_line_to_points` (`smgu_BestFitLineToPoints`) takes the unit eigenvector of the largest eigenvalue of the scatter matrix `M = sum Y_i Y_i^T`, the direction that minimises the sum of squared orthogonal distances. `best_fit_plane_to_points` and `PointSet3d`'s axes use it. This is now the only path, and the flag is gone. Removed: the field, the `_with_kernel` and `_opt_in` variants of both fits, `PointSet3d::new_with_kernel`, `PointSequence::new_with_kernel`, `PointSet3d::set_best_fit_line_fix`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

Collinear point sets are now classified Linear. `SetUpBBoxes` ran the tight pseudo-box's hull-edge alignment only for Planar sets, and upstream reached collinear sets with it only because its skewed line misclassified them as Planar. solidcore now runs the alignment for Linear sets too. The alignment keeps a rotated box only when its base area is smaller, so it never loosens a box.

- **Results** (checked against the scatter matrix rebuilt from each case's inputs):
  - `geom.geom_utility_p1`, 84 rows: the line direction minimises the squared residual in 81 of 85 changed results. Upstream's minimises it in none. The other 4 are sets with coordinates near 1e-11, where solidcore's residual is still the lower.
  - `geom.geom_utility_p2`, 29 rows: Planar becomes Linear. Every point lies within tolerance of the new line.
  - `geom.point_set`, 111 rows: a NaN normal becomes finite, and a non-optimal normal becomes optimal. None of the 1378 tight boxes is larger than either the upstream box or the box from the line fix without the Linear alignment; 76 are smaller than one of them and 2 are smaller than both. All changed boxes hold their points.
- **Tests:** `geom::geom_utility::p1` (`wr10_axis_aligned_collinear_points`, `wr10_rectangle_line_minimises_squared_residual`, `wr10_general_oblique_collinear_and_noisy_line`), `geom::geom_utility::p2` (`wr10_plane_fit_collinear_planar_and_scattered`) and `geom::point_set` (`wr10_point_set_collinear_axes`, `wr10_linear_set_tight_box_aligns_with_the_line`).
