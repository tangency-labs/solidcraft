Updates: WR-10

#### WR-10 (update, 2026-10-31). Opt-in largest scatter eigenvector for `smgu_BestFitLineToPoints` (`#2154`)

- **Location:** `source/SMLib/src/SmGeomUtility.cpp:4321`-`4361`, `smgu_BestFitLineToPoints` (def. `:4223`), called from `smgu_BestFitPlaneToPoints` (`:4513`) and `SmPointSet3d::SetUpAxes` (`source/SMLib/src/SmPointSet.cpp:334`).
- **What happens (default, pinned):** minimising the sum of squared orthogonal distances $\sum_i (\|Y_i\|^2 - (Y_i \cdot v)^2) = \operatorname{tr}(M) - v^T M v$ over unit $v$ requires maximising $v^T M v$ on the symmetric scatter matrix $M = \sum_i Y_i Y_i^T$ around the centroid (`rLinePt`), i.e. taking `M`'s largest-eigenvalue eigenvector (`vectors[2]` of `Compute3x3EigenVectors`, which sorts eigenvalues ascending). Pinned v1.0.1 instead has two compounding defects:
  1. **Un-negated off-diagonals (`:4324`-`:4334`):** builds `sC` with `del - M[i][i]` on the diagonal (`del = tr(M)`) while copying `+M[i][j]` instead of `-M[i][j]` into all six off-diagonal slots, so `sC` is not `del * I - M` whenever any off-diagonal entry is non-zero.
  2. **Wrong extreme of `sC` (`:4343`-`:4347`):** selects `aEigenVectors[2]` (largest eigenvalue of `sC`) instead of the smallest. For axis-aligned collinear points (e.g. along X with `M = diag(S, 0, 0)` and `sC = diag(0, S, S)`), `aEigenVectors[2]` is `(0, 0, 1)`, perpendicular to the line, yielding `SM_PST_SCATTERED` in `smgu_BestFitLineToPoints` and `SM_PST_PLANAR` in `smgu_BestFitPlaneToPoints`; for general oblique collinear lines with unequal coordinate magnitudes (e.g. along `(2, 1, -3)`), the sign-corrupted `sC` yields a skewed direction and also misclassifies the line as `Scattered` / `Planar`.
- **Opt-in:** `Kernel::fixes.best_fit_line_largest_scatter_eigenvector`, default `false` (`#2154`), exposed via:
  - `geom::geom_utility::p1::{best_fit_line_to_points_opt_in, best_fit_line_to_points_with_kernel}`: clones `M` before calling `Matrix::compute_3x3_eigen_vectors` so `BestFitLine::m` remains unmodified for planar fallback, and unitizes `vectors[2]` (largest scatter eigenvalue) as `line_vec`.
  - `geom::geom_utility::p2::{best_fit_plane_to_points_opt_in, best_fit_plane_to_points_with_kernel}`: forwards the opt-in to `best_fit_line_to_points_opt_in`, returning `PointSetType::Linear` with the line tangent for collinear sets while preserving `M` for planar and 3D scattered sets.
  - `geom::point_set::{PointSet3d::new_with_kernel, PointSet3d::set_best_fit_line_fix, PointSequence::new_with_kernel}`: forwards the setting from `SetUpAxes` to `best_fit_plane_to_points_opt_in`.
  - Default entry points (`best_fit_line_to_points`, `best_fit_plane_to_points`, `PointSet3d::new`, `PointSequence::new`) are unchanged for pinned C++ parity.
- **Caller trace:** Outside `geom::geom_utility` and `geom::point_set`, `PolyBrep::calculate_bounding_box` (`crates/solidcore/src/query/poly.rs:3204`) has no `Kernel` parameter and keeps pinned parity, while `topology::trimming_tools` (`crates/solidcore/src/topology/trimming_tools.rs:1991`) passes an explicit `Some(plane_normal)` to `PointSequence::new` so `SetUpAxes` takes the user-normal branch without calling `smgu_BestFitPlaneToPoints`.
- **Tests:**
  - `crates/solidcore/src/geom/geom_utility/p1.rs`: `wr10_axis_aligned_collinear_points`, `wr10_general_oblique_collinear_and_noisy_line`
  - `crates/solidcore/src/geom/geom_utility/p2.rs`: `wr10_plane_fit_collinear_planar_and_scattered`
  - `crates/solidcore/src/geom/point_set.rs`: `wr10_point_set_axes_and_flag_lookup`

`addendum_options: Kernel::fixes.best_fit_line_largest_scatter_eigenvector`
