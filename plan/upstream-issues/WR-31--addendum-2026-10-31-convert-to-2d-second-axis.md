Updates: WR-31

#### WR-31 addendum (2026-10-31): opt-in orthogonal second semi-axis in `SmEllipse::ConvertTo2D`

`Kernel::fixes.ellipse_convert_to_2d_uses_both_axes` (#2140) fixes three interacting
defects in `SmEllipse::ConvertTo2D` (`Ellipse::convert_to_2d`) when the projected axes
`X` and `Y` in the XY plane are not perpendicular (`|X . Y| >= SM_EFF_ZERO`). It is
`false` by default, preserving pinned C++ v1.0.1 bit-parity across all callers and
difftests.

1. **Quadratic coefficient order (`SmEllipse.cpp:1730-1734` vs `SmPolynomial.cpp:18-26`).**
   Upstream derives `0 = -(ry*rx*(X.Y))*t^2 + (ry^2*(Y.Y) - rx^2*(X.X))*t + (rx*ry*(X.Y))`
   for `t = tan(theta)`, then passes `[-(ry*rx*(X.Y)), B, +(ry*rx*(X.Y))]` (descending
   `[A, B, C]`) to `SmPolynomial::SolveQuadraticEqn`, which takes ascending `[C, B, A]`
   (`coefs[2]*t^2 + coefs[1]*t + coefs[0] = 0`). Reversing `coefs[0]` and `coefs[2]`
   without negating `coefs[1]` negates both roots whenever `ry^2*(Y.Y) != rx^2*(X.X)`,
   producing non-extremal, non-perpendicular vectors. Under the opt-in fix, `coefs` is
   `[ry * rx * x_dot_y, ry * ry * y_dot_y - rx * rx * x_dot_x, -ry * rx * x_dot_y]`.
2. **Second semi-axis angle (`SmEllipse.cpp:1737-1742`) and `SetCanonical` fallback.**
   Upstream computes `dThetaYRad = atan(adSols[1])` (shifted by `+PI` when `< dThetaXRad`)
   but evaluates both `sNewX` and `sNewY` at `dThetaXRad`. That sets
   `m_dRadiusAtYAxis = m_dRadiusAtXAxis` and passes parallel vectors `sNewX == sNewY` to
   `SmAxis2Placement::SetCanonical`, whose `MakeUnitOrthoVectors` fallback drops `z_axis`
   orientation (`X x (0, 1, 0)`) or, when `sNewX = (0, +-1, 0)`, falls back to
   `X x (0, 0, 1)` and leaves `y_axis = (0, 0, 1)` out of the XY plane. When the equation
   is linear (`sols.len() == 1`, e.g. `ry == 0` with `|X . Y| >= SM_EFF_ZERO`), upstream
   also reads uninitialised `adSols[1]` (UB-57). Under the opt-in fix, `sNewY` uses
   `dThetaYRad` from `sols[1]` when two roots exist and `theta_x_rad + HALF_PI` when one
   root exists, preserving both `+Z` and `-Z` orientations (`v x w = rx * ry * (X x Y)`).
3. **Full-turn STEP domain seam alignment (`SmEllipse.cpp:209-214, 1784`).**
   `SmPolarConversion::SetUpPolarConversion` only re-derives `m_vAnalDomain` from the NURB
   endpoints when `m_vAnalDomain.GetLength() < 360.0 - SM_EFF_ZERO`. On a closed full-turn
   ellipse (`[0, 360]`), rotating `XAxis` by `dThetaXRad != 0` moves the closed NURB seam
   away from `0`/`360` to an interior angle `seam_deg`, causing `SetUpPolarConversion` to
   record non-monotonic span angles and collapse `m_vAnalDomain` to `[seam_deg, seam_deg]`.
   Under the opt-in fix, full-turn non-degenerate ellipses with a NURB align `anal_domain`
   to `[seam_deg - 360, seam_deg]` in `[-360, 360]` (snapping back within
   `1000 * SM_EFF_ZERO` when unchanged) before rebuilding the polar converter.

- **Code:** `crates/solidcore/src/kernel.rs` (`ellipse_convert_to_2d_uses_both_axes`),
  `crates/solidcore/src/curve/ellipse/p1.rs` (`Ellipse::convert_to_2d`).
- **Tests:** `crates/solidcore/src/curve/ellipse/p1.rs::tests::wr31_*`
  (`wr31_tilted_ellipse_default_gives_equal_radii_and_mismatches_projection`,
  `wr31_tilted_ellipse_opt_in_matches_xy_projection_both_orientations`,
  `wr31_tilted_circle_projection_unequal_semi_axes`,
  `wr31_perpendicular_and_edge_on_degenerate_projections`).
