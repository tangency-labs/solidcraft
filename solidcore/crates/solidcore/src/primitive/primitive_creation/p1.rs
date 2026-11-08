//! Primitive creation state, planar-path helpers and primitive entry points.
//!
//! Upstream: `source/SMLib/inc/SmPrimitiveCreation.h`,
//! `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation`] | `SmPrimitiveCreation` |
//! | [`EntityMaps`] | the four `m_vMap*` members |
//! | [`PrimitiveCreation::init_entity_maps`] | `InitEntityMaps` |
//! | [`PrimitiveCreation::copy_entity_maps`] | `CopyEntityMaps` |
//! | [`find_pivot_point`] | `sm_FindPivotPoint` |
//! | [`extend_path_curves`] | `ExtendPathCurves` |
//! | [`position_profile_curves`] | `PositionProfileCurves` |
//! | [`offset_profile`] | static `OffsetProfile` |
//!
//! Deviations: owned curves and typed arena ids replace pointers. The four maps
//! use ordered maps; comparisons observe counts and lookups, never address hash order.
//! `CopyEntityMaps` copies FROM the primitive TO the sweep, despite its name.
//! Ignored statuses retain the previous output and propagate only Unported; with
//! `Kernel::fixes.planar_path_profile_placement_statuses`, `position_profile_curves`
//! returns the first failed box, normal, evaluation or placement status instead (WR-632, #483).
//! `primitive_path_curve_statuses` controls the kernel-aware pivot/extension variants;
//! the original signatures retain pinned behavior. The `SetCanonical` of
//! an identity profile-normal frame is elided (it always succeeds upstream).
//! `planar_path_profile_in_path_frame` performs placement in the whole path's own
//! XY frame (WR-660); XY and straight paths keep upstream arithmetic.
//! No recursion. Primitive topology construction stops at the first unported
//! B-rep edit; it never fabricates faces from arena records.

mod boolean;
mod offset_profile;
mod primitives;

pub use offset_profile::offset_profile;
pub use primitives::{analytic_rotational_sweep, create_circle, create_rectangle, Boolean2d};

use std::collections::BTreeMap;

use crate::curve::curve::p2::Curve;
use crate::curve::curve::p3::compute_crvs_normal;
use crate::error::{checked_or_ignored, ignore_status, ignored, EntityRef, Result};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::crv_adv::p1::CurveEnd;
use crate::topology::RegionId;

/// Four topology associations of a primitive or topology sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityMaps {
    /// Original entity to its translated counterpart.
    pub to_same: BTreeMap<EntityRef, EntityRef>,
    /// Original entity to the entity swept from it.
    pub to_higher: BTreeMap<EntityRef, EntityRef>,
    /// Translated entity to its original.
    pub from_same: BTreeMap<EntityRef, EntityRef>,
    /// Swept entity to its generating entity.
    pub from_lower: BTreeMap<EntityRef, EntityRef>,
}

/// Target region, tag and associations of `SmPrimitiveCreation`.
#[derive(Debug, Clone, Default)]
pub struct PrimitiveCreation {
    region: Option<RegionId>,
    tag_id: i64,
    /// Topology associations from the latest operation.
    pub maps: EntityMaps,
}

impl PrimitiveCreation {
    /// Constructor. `use_analytics` is unused upstream too.
    #[must_use]
    pub fn new(region: Option<RegionId>, tag_id: i64, _use_analytics: bool) -> Self {
        Self {
            region,
            tag_id,
            maps: EntityMaps::default(),
        }
    }

    /// The target region (`NULL` for the protected default constructor).
    #[must_use]
    pub const fn region(&self) -> Option<RegionId> {
        self.region
    }

    /// The caller's tag (`0` means no tagging).
    #[must_use]
    pub const fn tag_id(&self) -> i64 {
        self.tag_id
    }

    /// Change the target region (`Become`).
    pub fn become_region(&mut self, region: Option<RegionId>) {
        self.region = region;
    }

    /// Empty all four maps, retaining the region and tag.
    pub fn init_entity_maps(&mut self) {
        self.maps = EntityMaps::default();
    }

    /// Replace the sweep's maps with this primitive's maps (`CopyEntityMaps`).
    pub fn copy_entity_maps(&self, sweep: &mut EntityMaps) {
        sweep.clone_from(&self.maps);
    }
}

/// Join point of two links. Upstream compares squared distance with `tol`,
/// without squaring that tolerance; otherwise it returns the second curve's end.
/// Failed evaluations keep the default undefined point.
pub fn find_pivot_point(first: &BSplineCurve, second: &BSplineCurve, tol: f64) -> Result<Point3> {
    find_pivot_point_impl(first, second, tol, false)
}

/// Kernel-aware pivot selection. With `primitive_path_curve_statuses`, failed
/// evaluations propagate before comparing or returning their outputs.
pub fn find_pivot_point_with_kernel(
    k: &Kernel,
    first: &BSplineCurve,
    second: &BSplineCurve,
    tol: f64,
) -> Result<Point3> {
    find_pivot_point_impl(first, second, tol, k.fixes.primitive_path_curve_statuses)
}

fn find_pivot_point_impl(
    first: &BSplineCurve,
    second: &BSplineCurve,
    tol: f64,
    checked: bool,
) -> Result<Point3> {
    let a = first.natural_interval();
    let b = second.natural_interval();
    let start1 = checked_or_ignored(first.evaluate_point(a.min()), checked)?.unwrap_or(Vec3::UNDEF);
    let start2 =
        checked_or_ignored(second.evaluate_point(b.min()), checked)?.unwrap_or(Vec3::UNDEF);
    if (start1 - start2).length_squared() < tol {
        return Ok(start2);
    }
    let end1 = checked_or_ignored(first.evaluate_point(a.max()), checked)?.unwrap_or(Vec3::UNDEF);
    if (end1 - start2).length_squared() < tol {
        return Ok(start2);
    }
    Ok(checked_or_ignored(second.evaluate_point(b.max()), checked)?.unwrap_or(Vec3::UNDEF))
}

fn evaluate_keep(
    curve: &BSplineCurve,
    t: f64,
    from_left: bool,
    out: &mut [Vec3; 2],
    checked: bool,
) -> Result<()> {
    if let Some(values) = checked_or_ignored(curve.evaluate(t, 1, from_left, false), checked)? {
        for (target, value) in out.iter_mut().zip(values) {
            *target = value;
        }
    }
    Ok(())
}

fn extend(curve: &mut BSplineCurve, distance: f64, end: CurveEnd, checked: bool) -> Result<()> {
    if let Some(Some(next)) = checked_or_ignored(
        curve.create_extended_curve_by_distance(distance, end, ContinuityType::G1),
        checked,
    )? {
        *curve = next;
    }
    Ok(())
}

/// Extend non-parallel path joins by arc length. The original input list stays
/// unchanged: each output replaces the local pointer upstream, while neighbours
/// are always read from the original list. The derivative buffers persist across
/// iterations, just as the two upstream stack arrays do.
pub fn extend_path_curves(
    paths: &[BSplineCurve],
    closed: bool,
    periodic: bool,
    distance: f64,
) -> Result<Vec<BSplineCurve>> {
    extend_path_curves_impl(paths, closed, periodic, distance, false)
}

/// Kernel-aware path extension. Failed evaluations and extensions propagate with
/// `primitive_path_curve_statuses`; the input curves remain unchanged.
pub fn extend_path_curves_with_kernel(
    k: &Kernel,
    paths: &[BSplineCurve],
    closed: bool,
    periodic: bool,
    distance: f64,
) -> Result<Vec<BSplineCurve>> {
    extend_path_curves_impl(
        paths,
        closed,
        periodic,
        distance,
        k.fixes.primitive_path_curve_statuses,
    )
}

pub(super) fn extend_path_curves_impl(
    paths: &[BSplineCurve],
    closed: bool,
    periodic: bool,
    distance: f64,
    checked: bool,
) -> Result<Vec<BSplineCurve>> {
    let mut output = Vec::new();
    let mut before = [Vec3::UNDEF; 2];
    let mut after = [Vec3::UNDEF; 2];
    for (index, source) in paths.iter().enumerate() {
        let mut path = source.clone();
        if let Some(previous) = index.checked_sub(1).and_then(|i| paths.get(i)) {
            evaluate_keep(
                previous,
                previous.natural_interval().max(),
                true,
                &mut before,
                checked,
            )?;
            evaluate_keep(
                &path,
                path.natural_interval().min(),
                false,
                &mut after,
                checked,
            )?;
            if !before
                .get(1)
                .copied()
                .unwrap_or(Vec3::UNDEF)
                .is_parallel_to(after.get(1).copied().unwrap_or(Vec3::UNDEF), 2.0)
            {
                extend(&mut path, distance, CurveEnd::Start, checked)?;
            }
        } else if closed && !periodic {
            extend(&mut path, distance, CurveEnd::Start, checked)?;
        }
        if let Some(next) = paths.get(index + 1) {
            evaluate_keep(
                &path,
                path.natural_interval().max(),
                true,
                &mut before,
                checked,
            )?;
            evaluate_keep(
                next,
                next.natural_interval().min(),
                false,
                &mut after,
                checked,
            )?;
            if !before
                .get(1)
                .copied()
                .unwrap_or(Vec3::UNDEF)
                .is_parallel_to(after.get(1).copied().unwrap_or(Vec3::UNDEF), 2.0)
            {
                extend(&mut path, distance, CurveEnd::End, checked)?;
            }
        } else if closed && !periodic {
            extend(&mut path, distance, CurveEnd::End, checked)?;
        }
        output.push(path);
    }
    Ok(output)
}

/// Copy and position a profile at the path start (`PositionProfileCurves`).
/// The tolerance is unused upstream. Copies are appended before transforming
/// the entire output list, including any entries supplied by the caller.
///
/// With `Kernel::fixes.planar_path_profile_placement_statuses` the first failing
/// status of bounding boxes, profile normals, angles, path evaluations, the two
/// `SetCanonical` calls, the three transforms or `MakeUnitOrthoVectors` is returned (WR-632, #483); the edits
/// already made stay, and the partial axes are not used after a failed
/// `MakeUnitOrthoVectors`. Off by default, the statuses are ignored as upstream does.
///
/// With `Kernel::fixes.planar_path_profile_in_path_frame`, paths with a plane outside
/// XY place the profile in that plane's frame at the path's start and map it back
/// (WR-660). Paths parallel to XY keep the exact legacy placement, with either normal,
/// and straight paths the legacy placement.
///
/// # Errors
///
/// The path-frame opt-in propagates failed frame evaluations and transforms.
/// Copy failures and unported callees propagate. With the placement-status opt-in,
/// failures of the box, normal, angle, evaluation, placement and transform calls propagate,
/// without rolling back earlier edits.
pub fn position_profile_curves(
    k: &Kernel,
    profiles: &[Curve],
    tol: f64,
    path: &Curve,
    output: &mut Vec<Curve>,
) -> Result<()> {
    position_profile_curves_impl(
        k,
        profiles,
        tol,
        path,
        output,
        k.fixes.planar_path_profile_placement_statuses,
        std::slice::from_ref(path),
    )
}

pub(super) fn position_profile_curves_impl(
    k: &Kernel,
    profiles: &[Curve],
    tol: f64,
    path: &Curve,
    output: &mut Vec<Curve>,
    checked: bool,
    whole_path: &[Curve],
) -> Result<()> {
    // WR-660: perform the legacy placement in the whole path's own XY frame, one frame
    // for all its pieces. Both of its rotations use global axes; changing just the final
    // Y axis leaves the profile-normal rotation (and its roll) pose dependent.
    if k.fixes.planar_path_profile_in_path_frame {
        // No normal: a straight path does not determine a plane.
        if let Ok(normal) = compute_crvs_normal(k, whole_path) {
            // Preserve upstream arithmetic for paths in or parallel to XY,
            // regardless of the direction in which the path turns.
            if normal.x.abs() > crate::math::EFF_ZERO || normal.y.abs() > crate::math::EFF_ZERO {
                let no_start =
                    || crate::error::SolidError::sm_err("primitive::position_profile_curves");
                let first = whole_path.first().ok_or_else(no_start)?;
                let start = first.evaluate(k, first.natural_interval().min(), 1, true, true)?;
                let origin = start.first().copied().ok_or_else(no_start)?;
                let tangent = start.get(1).copied().ok_or_else(no_start)?.unitize()?;
                let x = tangent.cross(normal).unitize()?;
                let frame = Axis2Placement::new(origin, x, tangent);
                let inverse = frame.invert();
                let mut local_profiles = profiles.to_vec();
                for curve in &mut local_profiles {
                    curve.transform(k, &inverse, None)?;
                }
                let mut local_path = path.clone();
                local_path.transform(k, &inverse, None)?;
                let mut local_output = output.clone();
                for curve in &mut local_output {
                    curve.transform(k, &inverse, None)?;
                }
                position_profile_curves_legacy(
                    k,
                    &local_profiles,
                    tol,
                    &local_path,
                    &mut local_output,
                    checked,
                )?;
                for curve in &mut local_output {
                    curve.transform(k, &frame, None)?;
                }
                *output = local_output;
                return Ok(());
            }
        }
    }
    position_profile_curves_legacy(k, profiles, tol, path, output, checked)
}

fn position_profile_curves_legacy(
    k: &Kernel,
    profiles: &[Curve],
    _tol: f64,
    path: &Curve,
    output: &mut Vec<Curve>,
    checked: bool,
) -> Result<()> {
    let mut total = Extent3d::new();
    let mut bounds = Extent3d::new();
    for profile in profiles {
        if let Some(boxes) = checked_or_ignored(
            profile.calculate_bounding_box(
                k,
                &profile.natural_interval(),
                BoundingBoxRequest {
                    normal: true,
                    ..BoundingBoxRequest::default()
                },
                true,
            ),
            checked,
        )? {
            if let Some(normal) = boxes.normal {
                bounds = normal;
            }
        }
        total.add_point(bounds.min());
        total.add_point(bounds.max());
        // A failed Copy produces NULL upstream, then Transform dereferences it.
        output.push(profile.copy(k)?);
    }
    let mut translation = Axis2Placement::IDENTITY;
    if checked {
        translation.set_canonical(-total.mid(), Vec3::X, Vec3::Y)?;
    } else {
        ignore_status(translation.set_canonical(-total.mid(), Vec3::X, Vec3::Y))?;
    }
    for curve in output.iter_mut() {
        let status = curve.transform(k, &translation, None);
        if checked {
            status?;
        } else {
            ignore_status(status)?;
        }
    }
    let normal =
        checked_or_ignored(compute_crvs_normal(k, profiles), checked)?.unwrap_or(Vec3::UNDEF);
    let mut orientation = Axis2Placement::IDENTITY;
    let angle = checked_or_ignored(Vec3::X.angle_between(normal), checked)?.unwrap_or(0.0);
    orientation.rotate_about_axis(-angle, Vec3::X.cross(normal));
    for curve in output.iter_mut() {
        let status = curve.transform(k, &orientation, None);
        if checked {
            status?;
        } else {
            ignore_status(status)?;
        }
    }
    let mut start = [Vec3::UNDEF; 2];
    if let Some(values) = checked_or_ignored(
        path.evaluate(k, path.natural_interval().min(), 1, true, true),
        checked,
    )? {
        for (target, value) in start.iter_mut().zip(values) {
            *target = value;
        }
    }
    let origin = start.first().copied().unwrap_or(Vec3::UNDEF);
    let tangent = start.get(1).copied().unwrap_or(Vec3::UNDEF);
    let mut y = Vec3::Z.cross(tangent);
    if tangent.z.abs() > tangent.x.abs() && tangent.z.abs() > tangent.y.abs() {
        y = Vec3::Y;
    }
    // MakeUnitOrthoVectors writes/unitizes X before any later failure.
    // Its caller ignores that status but still uses the partially written axes.
    let mut x = tangent.unitize_or_z();
    let ortho = tangent.make_unit_ortho_vectors(Some(y));
    if checked {
        let [next_x, next_y, _] = ortho?;
        x = next_x;
        y = next_y;
    } else if let Some([next_x, next_y, _]) = ignored(ortho)? {
        x = next_x;
        y = next_y;
    }
    let mut placement = Axis2Placement::IDENTITY;
    if checked {
        placement.set_canonical(origin, x, y)?;
    } else {
        ignore_status(placement.set_canonical(origin, x, y))?;
    }
    for curve in output.iter_mut() {
        let status = curve.transform(k, &placement, None);
        if checked {
            status?;
        } else {
            ignore_status(status)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_483_pivot_and_extensions_stop_at_failed_evaluations() {
        let empty = BSplineCurve::default();
        let line = BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X).unwrap();
        let mut k = Kernel::default();
        assert!(!k.fixes.primitive_path_curve_statuses);
        assert_eq!(find_pivot_point(&empty, &line, 1e-9).unwrap(), Vec3::ZERO);
        assert!(extend_path_curves(&[empty.clone(), line.clone()], false, false, 1.0).is_ok());
        k.fixes.primitive_path_curve_statuses = true;
        let error = empty
            .evaluate_point(empty.natural_interval().min())
            .unwrap_err();
        assert_eq!(
            find_pivot_point_with_kernel(&k, &empty, &line, 1e-9).unwrap_err(),
            error
        );
        let error = empty
            .evaluate(empty.natural_interval().max(), 1, true, false)
            .unwrap_err();
        assert_eq!(
            extend_path_curves_with_kernel(&k, &[empty, line.clone()], false, false, 1.0)
                .unwrap_err(),
            error
        );
        assert_eq!(
            find_pivot_point_with_kernel(&k, &line, &line, 1e-9).unwrap(),
            Vec3::ZERO
        );
    }

    #[test]
    fn audit_483_placement_read_failures_keep_the_successful_prefix() {
        let mut k = Kernel::default();
        k.fixes.planar_path_profile_placement_statuses = true;
        let profile =
            Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X).unwrap());
        let path = Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::Z).unwrap());
        let expected = compute_crvs_normal(&k, std::slice::from_ref(&profile)).unwrap_err();
        let mut output = Vec::new();
        assert_eq!(
            position_profile_curves(&k, &[profile], 1e-9, &path, &mut output).unwrap_err(),
            expected
        );
        assert_eq!(output.len(), 1);
        assert_eq!(
            output[0].evaluate_point(&k, 0.0).unwrap(),
            Vec3::new(-0.5, 0.0, 0.0)
        );
    }

    #[test]
    fn audit_483_placement_rejects_failed_box_before_appending_copy() {
        let mut k = Kernel::default();
        k.fixes.planar_path_profile_placement_statuses = true;
        let bad = Curve::from(BSplineCurve::default());
        let path = Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::Z).unwrap());
        let expected = bad
            .calculate_bounding_box(
                &k,
                &bad.natural_interval(),
                BoundingBoxRequest {
                    normal: true,
                    ..Default::default()
                },
                true,
            )
            .unwrap_err();
        let mut output = vec![path.clone()];
        let result = position_profile_curves(&k, &[bad], 1e-9, &path, &mut output);
        assert_eq!(result.unwrap_err(), expected);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].evaluate_point(&k, 0.0).unwrap(), Vec3::ZERO);
    }

    use crate::error::SolidError;
    use crate::nurbs::b_spline_curve::p1::NurbCircleParam;

    type PlacementCase = (f64, [f64; 3], [f64; 3], [f64; 3], f64);

    /// The four `overflow_regression_*` profile placements of
    /// `fixtures/primitive.primitive_creation_p1/overflow.json`: `(tol, path start,
    /// path end, profile circle centre, profile radius)`. Each path start tangent has a
    /// `1e300` component, so its squared length overflows and `Unitize` underflows to
    /// zero, which makes `MakeUnitOrthoVectors` fail `SM_ERR`.
    const WR632_CASES: [PlacementCase; 4] = [
        (
            -0.069_325_498_820_103_3,
            [-1e300, 4.088_518_620_795_586, -2.376_803_691_591_771],
            [
                8.077_343_844_368_341,
                6.980_437_345_869_189_5,
                7.320_032_911_063_913,
            ],
            [
                -0.376_002_347_473_951_56,
                5.821_770_650_932_066,
                8.145_188_887_399_406,
            ],
            1.357_225_096_972_516_8,
        ),
        (
            -0.409_285_870_604_335_2,
            [
                -3.871_788_481_168_059,
                -2.426_025_440_916_085,
                -3.661_284_586_455_48,
            ],
            [7.219_749_520_839_942, 9.192_338_292_919_843, -1e300],
            [
                0.863_890_823_361_739_3,
                1.465_661_568_387_297,
                7.548_857_400_571_528,
            ],
            3.251_486_071_077_096,
        ),
        (
            -0.042_746_787_759_025_96,
            [-1.717_673_732_024_804, -1e300, 0.935_719_246_946_193],
            [
                9.039_050_828_860_168,
                6.705_299_766_279_547_5,
                6.208_417_859_911_531,
            ],
            [
                6.977_017_153_053_087,
                -7.350_041_583_436_151,
                -9.693_389_675_986_875,
            ],
            1.882_199_741_104_367_4,
        ),
        (
            -0.127_695_474_602_225_4,
            [
                0.146_017_618_101_694_78,
                -0.090_930_075_154_402_88,
                2.455_375_303_305_535_4,
            ],
            [6.960_420_164_133_81, 8.610_376_323_133_508, -1e300],
            [
                7.324_085_420_004_415,
                2.267_170_066_085_890_7,
                -2.984_613_673_153_746_5,
            ],
            2.914_330_324_270_461_7,
        ),
    ];

    fn path_curve(start: [f64; 3], end: [f64; 3]) -> Curve {
        let [sx, sy, sz] = start;
        let [ex, ey, ez] = end;
        Curve::from(
            BSplineCurve::create_line_segment(3, Point3::new(sx, sy, sz), Point3::new(ex, ey, ez))
                .expect("path line"),
        )
    }

    fn circle_profile(centre: [f64; 3], radius: f64) -> Curve {
        let [x, y, z] = centre;
        Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::new(Point3::new(x, y, z), Vec3::X, Vec3::Y),
                radius,
                0.0,
                360.0,
                NurbCircleParam::Quadratic,
            )
            .expect("profile circle"),
        )
    }

    fn marker() -> Curve {
        path_curve([10.0, 0.0, 0.0], [11.0, 0.0, 0.0])
    }

    fn samples(k: &Kernel, curve: &Curve) -> Vec<Vec3> {
        let interval = curve.natural_interval();
        (0..=4)
            .map(|i| {
                let values = curve
                    .evaluate(k, interval.evaluate(f64::from(i) / 4.0), 1, true, true)
                    .expect("sample");
                *values.first().expect("position")
            })
            .collect()
    }

    /// The pinned C++ ignores the placement statuses; the opt-in returns the first one
    /// (`MakeUnitOrthoVectors`, after the two transforms that precede it) and leaves the
    /// edits already made, including the caller's pre-existing output entries.
    #[test]
    fn wr632_opt_in_returns_first_failed_placement_status_and_keeps_edits() {
        for (tol, start, end, centre, radius) in WR632_CASES {
            let path = path_curve(start, end);
            let profiles = [circle_profile(centre, radius)];
            let before = samples(&Kernel::default(), &profiles[0]);
            let marker_before = samples(&Kernel::default(), &marker());

            // Default: pinned C++ behaviour, which carries on with the partial frame.
            let mut pinned = vec![marker()];
            position_profile_curves(&Kernel::default(), &profiles, tol, &path, &mut pinned)
                .expect("pinned C++ ignores the failed placement");
            assert_eq!(pinned.len(), 2);

            let mut k = Kernel::default();
            k.fixes.planar_path_profile_placement_statuses = true;
            let mut output = vec![marker()];
            let error = position_profile_curves(&k, &profiles, tol, &path, &mut output)
                .expect_err("opt-in reports the failed placement");
            assert_eq!(
                error,
                SolidError::sm_err("geom::vector3d::Vec3::make_unit_ortho_vectors"),
                "unexpected first error for {start:?} -> {end:?}"
            );

            // The copies were appended after the caller's entry, and both earlier
            // transforms ran on the whole list before the failure: the profile circle
            // now sits at the origin, not at its fixture centre.
            assert_eq!(output.len(), 2);
            let placed = samples(&k, &output[1]);
            for point in &placed {
                assert!(
                    (point.length() - radius).abs() <= 1e-9 * radius,
                    "profile not translated to the origin: {point:?}"
                );
            }
            assert!(
                Point3::new(centre[0], centre[1], centre[2]).length() > 2.0 * radius,
                "fixture centre too close to the origin to prove the translation"
            );
            assert_ne!(placed, before, "the profile copy was not transformed");
            assert_ne!(
                samples(&k, &output[0]),
                marker_before,
                "the caller's pre-existing output was not transformed"
            );
        }
    }

    /// A well-conditioned path and profile place identically with the opt-in: the flag
    /// only changes what happens when one of the placement calls fails.
    #[test]
    fn wr632_opt_in_leaves_a_well_conditioned_placement_unchanged() {
        let path = path_curve([0.0, 0.0, 0.0], [0.0, 0.0, 5.0]);
        let profiles = [circle_profile([1.0, 2.0, 0.0], 0.5)];

        let mut pinned = Vec::new();
        position_profile_curves(&Kernel::default(), &profiles, 1e-9, &path, &mut pinned)
            .expect("pinned placement");

        let mut k = Kernel::default();
        k.fixes.planar_path_profile_placement_statuses = true;
        let mut opt_in = Vec::new();
        position_profile_curves(&k, &profiles, 1e-9, &path, &mut opt_in)
            .expect("well-conditioned opt-in placement");

        assert_eq!(pinned.len(), opt_in.len());
        for (a, b) in pinned.iter().zip(&opt_in) {
            assert_eq!(a.natural_interval(), b.natural_interval());
            for sample in 0..=4 {
                let fraction = f64::from(sample) / 4.0;
                assert_eq!(
                    a.evaluate(&k, a.natural_interval().evaluate(fraction), 1, true, true)
                        .expect("pinned position and tangent"),
                    b.evaluate(&k, b.natural_interval().evaluate(fraction), 1, true, true)
                        .expect("opt-in position and tangent")
                );
            }
        }
    }
}
