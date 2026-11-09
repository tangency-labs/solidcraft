#### WR-14 (addendum 2026-11-01) — Opt-in plane projection in `SmPointSet3d::SetUpTightPseudoBox` (#2201)

Updates: WR-14

`Kernel::fixes.point_set_tight_pbox_projects_points` (#2201, default `false`) opts into
assigning the projected points returned by `Vec3::project_point_to_plane(plane_point, plane_z)`
in `PointSet3d::set_up_tight_pseudo_box_with_fixes` (`crates/solidcore/src/geom/point_set.rs`),
the Rust port of `SmPointSet3d::SetUpTightPseudoBox`
(`source/SMLib/src/SmPointSet.cpp:631-909`).

- **Root cause (`SmPointSet.cpp:651, 764-809`):** `SmVector3d::ProjectPointToPlane`
  (`source/SMLib/inc/SmVector3d.h:806-821`) is a `const` member returning the projected
  `SmPoint3d` by value without mutating `*this`. `SetUpTightPseudoBox` computes
  `sPlanePoint = m_sPBox.Evaluate(.5, .5, .5)` (`:651`) and updates
  `sPlaneZ = m_sPBox.GetBasis(2)` (`:766`), then calls
  `ProjectPointToPlane(sPlanePoint, sPlaneZ)` as a discarded statement on all four boundary
  points (`sBegPt0`, `sEndPt0`, `sBegPt1`, `sEndPt1` at `:777-780`) and on each loop candidate
  point (`sThisPt` at `:790`). As a result, `sBegVec0..sEndVec1` (`:800-803`) are 3D chords
  between unprojected points; when the point set has out-of-plane offsets along `sPlaneZ`,
  `(sPlaneNormal * vec) * sPlaneNormal` inside `CCWAngleBetween` (`:806-809`) amplifies
  out-of-plane components against floating-point residuals between `sPlaneNormal` and `sPlaneZ`,
  skewing the measured hull angles and potentially choosing a suboptimal hull edge.
- **Call-site audit:** Across `source/SMLib` at v1.0.1 (`3fd5b95`), every other
  `ProjectPointToPlane` / `ProjectToPlane` call assigns or returns its result.
  `SetUpTightPseudoBox` is called only from `SmPointSet3d::SetUpBBoxes` (`SmPointSet.cpp:614`),
  and `GetTightPBox` is called only from `SmPoly.cpp:2694, 2736` (`PolyBrep::calculate_bounding_box`
  in `crates/solidcore/src/query/poly.rs:3207`, which has no `&Kernel` parameter and preserves
  pinned C++ parity).
- **Default (`false`, pinned C++ parity):** `PointSet3d::new` + `PointSet3d::tight_pbox`
  leaves `beg_pt0`, `end_pt0`, `beg_pt1`, `end_pt1`, and `this_pt` unprojected, matching the
  pinned v1.0.1 C++ reference bit-for-bit.
- **Opt-in (`true`):** Exposed via `PointSet3d::tight_pbox_with_fixes`,
  `PointSet3d::tight_pbox_with_kernel`, `PointSet3d::new_with_kernel`,
  `PointSequence::new_with_kernel`, and `PointSet3d::set_tight_pbox_projection_fix`. Evaluates
  `plane_point = self.pbox.evaluate(0.5, 0.5, 0.5)` before the five trial rotations (`:651`)
  and assigns `p.project_point_to_plane(plane_point, plane_z)` for the four boundary points
  (`:777-780`) and each candidate point (`:790`) before forming `beg_vec0..end_vec1`. Switching
  modes invalidates `Cache::BBOX | Cache::PBOX_REFINED` so `self.pbox` is rebuilt cleanly from
  its unrotated axes.
- **Regression tests:** `crates/solidcore/src/geom/point_set.rs::tests`:
  - `wr14_flag_lookup_and_toggle` checks `UpstreamFixes::NAMES`, `UpstreamFixes::all()`,
    `flags()`, and `flag_mut("point_set_tight_pbox_projects_points")` toggling.
  - `wr14_nonplanar_point_set_projects_before_hull_angle_selection` verifies a 3D non-planar
    oblique point set where out-of-plane offsets skew unprojected chord angles onto a rejected
    chamfer (`xy_area > 75`, `volume > 6.0e7`), while opt-in projects onto the pseudo-box plane,
    rotates `basis[0]` by `0.08` rad onto the long hull edge, shrinks `xy_area` and `volume` by
    ~32% (`< 53` and `< 4.2e7`), preserves 3D point containment in both modes, and invalidates
    cached refined boxes across mode switches and `Some(&mut pbox_z)`.
  - `wr14_planar_point_sets_match_in_both_modes` checks exact equality on planar XY point sets
    (`z = 0` with `normal = None` and `z = 3.5` with explicit `+Z`) and floating-point agreement
    on an oblique planar set (`w = 7.5` along `(1, 2, 3)`).
