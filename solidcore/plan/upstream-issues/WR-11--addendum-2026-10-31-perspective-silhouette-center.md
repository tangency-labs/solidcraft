Updates: WR-11

#### WR-11 (update, 2026-10-31). Opt-in perspective circle silhouette points (`#2138`)

- **Location:** `source/SMLib/src/SmGeomUtility.cpp:2198`-`2225`, `smgu_CircleFindSilhouettePoints` (def. `:2185`), called from `SmSurfaceSilhouette::LocalPointSolve` (`source/SMLib/src/SmSurfaceSilhouette.cpp:846`).
- **What happens (default, pinned):** the perspective branch (`bPerspective == TRUE`) has two defects:
  1. **Reversed eye-direction sign (`:2202`):** `SmVector3d sToEye = crCircleCenter - sProjEyePoint` points away from the projected eye instead of toward `sProjEyePoint - crCircleCenter`. Even when `crCircleCenter == (0, 0, 0)`, rotating `sToEye` by `±acos(r / d)` places both returned points on the far side of the circle where `(P - E) · (P - C) == 2 * r^2 != 0` instead of `0`.
  2. **Rotated origin and missing centre offset (`:2214`-`2223`):** `sA2P.SetCanonical(crCircleCenter, X, Y)` followed by `RotateAboutAxis(±dAngleRad, crCircleNormal)` rotates `sA2P`'s origin `crCircleCenter` about the global origin `(0, 0, 0)`, `TransformPoint(sToEye, sToPi)` evaluates `R_±(crCircleCenter + sToEye)`, and `rSilPointi = sToPi * dCircleRadius` scales the whole sum by `dCircleRadius` without adding `crCircleCenter` (`r * R_±(C + sToEye)` instead of `C + r * R_±(toward_eye)`).
- **Opt-in:** `Kernel::fixes.circle_silhouette_perspective_center`, default `false` (`#2138`), exposed via `geom::geom_utility::p1::circle_find_silhouette_points_opt_in` and `geom::geom_utility::p1::circle_find_silhouette_points_with_kernel`.
  - When enabled in perspective, returns `Ok(None)` if `circle_normal` is degenerate or non-finite (`|N|^2 < SM_EFF_ZERO_SQ`), if `circle_radius` is negative or non-finite, or if `|proj_eye - circle_center| < circle_radius + SM_EFF_ZERO`; otherwise unitizes `toward_eye = proj_eye - circle_center`, rotates it as a pure vector about `circle_normal` by `±acos(circle_radius / length)` (`Axis2Placement::IDENTITY` + `rotate_about_axis` + `transform_vector`), and returns `circle_center + dir * circle_radius`.
  - Parallel projection (`perspective == false`) and the default `circle_find_silhouette_points` entry point are unchanged for pinned C++ parity.
- **Caller wiring:** `intersect::surface_silhouette::SurfaceSilhouette::local_point_solve` (`crates/solidcore/src/intersect/surface_silhouette.rs`) forwards `k: &Kernel` to `circle_find_silhouette_points_with_kernel`.
- **Tests:** `crates/solidcore/src/geom/geom_utility/p1.rs`:
  - `wr11_perspective_origin_circle_to_eye_sign`
  - `wr11_perspective_translated_tilted_circle_and_non_unit_normal`
  - `wr11_inside_radius_parallel_degenerate_and_kernel_wiring`

`addendum_options: Kernel::fixes.circle_silhouette_perspective_center`
