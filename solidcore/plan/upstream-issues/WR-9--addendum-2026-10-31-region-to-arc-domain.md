Updates: WR-9

#### WR-9 (update, 2026-10-31). Opt-in rotated domain for region-to-arc collapse

`SmPolarBox::AddVector3d` (`source/SMLib/src/SmPolarBox.cpp:696`-`922`, `SM_PS_REGION`
collapse at `:868`-`877`), when `fabs(m_sPolarDomain.XLength()) < SM_EFF_ZERO_SQRT` and
the box is not unbounded, rotates `(m_aBasis[1], m_aBasis[2])` to
`(m_aBasis[2], -m_aBasis[1])` and sets `m_ePolarBoxState = SM_PS_ARC`, but calls
`SetMinMax(GetUMin(), GetVMin(), GetUMax(), GetVMax())` (`:874`-`875`). That leaves the
arc's angular span in `y` (`v`) and the collapsed span in `x` (`u`), whereas
`SM_PS_ARC` operations (`EvaluateNormalized`, `GetBoundaryVectors`, `ExpandAbsolute`)
read `x` and `ComputePolarCoord` in the rotated basis maps old polar coordinates
`(u, v)` to `(v, -u)`.

- **Fix (`Kernel::fixes.polar_box_region_to_arc_transposes_domain`, #2133):** default
  `false` keeps upstream's `set_domain(lo.x, lo.y, hi.x, hi.y)` in
  `PolarBox::add_vector` for pinned C++ parity. When enabled (via
  `PolarBox::add_vector_with_kernel` or `PolarBox::add_vector_opt_in(..., true)`), the
  `PolarBoxState::Region` collapse sets the domain to `[lo.y, hi.y] × [-hi.x, -lo.x]`,
  putting the arc's span on `x` and keeping every previously added vector's rotated
  polar coordinates `(v, -u)` inside `domain`.
- **Wired callers:** `curve::curve::approximate_polar_box`, `curve::curve`'s base
  `calculate_bounding_box`, `BSplineSurface::calculate_bounding_box` (both the main box
  and `trial_polar_box` with `polar_guess`), and `surface::surface` plane/patch
  polar-box builders forward `k: &Kernel` to `add_vector_with_kernel`.
- **Regression tests (`geom::polar_box::tests`):**
  - `wr9_preset_basis_region_collapse_transposes_arc_domain`: preset `opt_basis2`
    (`PointWithBasis -> Region -> Arc`) shows default leaves `x` collapsed at `0` and
    `v1` outside `domain`, while opt-in spans `[-0.15, 0.25]` in `x` and round-trips all
    added vectors through `compute_polar_coord`, `domain.inversion`, and
    `evaluate_normalized`.
  - `wr9_nonzero_subthreshold_x_span_negates_into_y_bounds`: non-zero sub-threshold
    `u` range `[9.8e-6, 1.02e-5]` (`x_length = 4e-7 < EFF_ZERO_SQRT`) maps to `y` range
    `[-1.02e-5, -9.8e-6]`, keeping added vectors inside `domain` both in the collapsed
    `Arc` state and after a subsequent out-of-plane vector promotes the box back to
    `Region`.
  - `wr9_single_point_arc_region_collapse_and_kernel_flag`: `SinglePoint -> Arc ->
    Region -> Arc` without `opt_basis2`, verifying `Kernel::default()` matches
    `add_vector` and `polar_box_region_to_arc_transposes_domain = true` matches
    `add_vector_opt_in(..., true)`.
- **Limits:** callers that do not take `&Kernel` (`PolarBox::cross`, `PolarBox::union`,
  `PolarBox::union_assign`, `HermiteCurve::calculate_bounding_box`,
  `BSplineCurve::calculate_bounding_box_onto`, and
  `SurfaceSilhouette::patch_may_contain_answers`) continue to call `PolarBox::add_vector`
  unless called directly through `add_vector_opt_in` or `add_vector_with_kernel`.
