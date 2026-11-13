Updates: WR-58

#### WR-58 (update, 2026-11-01). Confirmed; opt-in error propagation for surface-projected `GetGap3d`

`SmPointClassification::GetGap3d(const SmSurface * pOptSurface)`
(`source/SMLib/src/SmPointClass.cpp:418`-`480`) computes the component of `sObjPt3d - sSurfPt3d`
perpendicular to `pOptSurface`'s normal at the object point's surface drop point when
`pOptSurface != NULL`:

1. `FindObjPoint3d`, both `pOptSurface->GlobalPointSolve(..., SM_SO_INTERSECT, ..., SM_SR_SINGLE, ...)`
   calls (at `1x` and `100x` `sObjZoneTol3d`), `EvaluatePoint`, and `EvaluateNormal` ignore
   failing `SmStatus` returns.
2. When `sSolutions.GetSize() != 1` still holds after the `100 * sObjZoneTol3d` retry (`:462`-`465`),
   upstream executes `return SM_ERR;` inside a `double`-returning function, implicitly converting
   `SM_ERR` (`1001L`) to `1001.0`, which callers such as `SmCurveClassification::HasNearMisses`
   (`SmCurveClass.cpp:13577`, `>= dNearMissDist3d`) treat as a huge 3D gap.

- **Confirmed:** yes, reproduced directly with the ported `Surface::global_point_solve` on bounded
  `Plane` and `BSplineSurface` patches for object points beyond `100 * sObjZoneTol3d`.
- **Rust port (#2215):** `Kernel::fixes.point_class_gap_3d_propagates_no_drop_error` (default
  `false` for pinned `v1.0.1` C++ parity, returning `Ok(1001.0)`) makes
  `PointClassification::gap_3d` return `Err(SolidError::sm_err(...))` (`SM_ERR`) when no single
  drop point is found even at `100 * obj_zone_tol_3d`, and propagates failing statuses from
  `find_obj_point_3d`, `surface_global_point_solve`, `evaluate_point`, and
  `surface_evaluate_normal` via `ignored_or_pass_on`.
- **Regression:** `wr58_*` unit tests in `crates/solidcore/src/intersect/point_class.rs` cover
  normal and lateral out-of-range points on bounded `Plane` and `BSplineSurface` patches, failing
  surface solves (NURB-less `BSplineSurface`) and unset object classifications, `1x` and `100x`
  in-range surface drops, `opt_surface = None` cached gaps, and
  `UpstreamFixes::flag_mut("point_class_gap_3d_propagates_no_drop_error")`.
