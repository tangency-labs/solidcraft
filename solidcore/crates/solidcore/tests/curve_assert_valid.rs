//! Regression: the `AssertValid` checks of the offset, projected, Hermite, curve-on-
//! surface and curve-in-volume curves (`SmOffsetCurve`, `SmProjectedCurve`,
//! `SmHermiteCurve`, `SmCrvOnSurf`, `SmCrvInVolume::AssertValid`) run instead of
//! returning `Unported`. Each kind has a valid case (no reports) and an invalid one
//! with the reports upstream's checks produce.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::{
    curve::{
        crv_in_volume::CrvInVolume,
        crv_on_surf::CrvOnSurf,
        curve::{p2 as types, Curve},
        hermite_curve::HermiteCurve,
        line::Line,
        offset_curve::OffsetCurve,
        projected_curve::{OwnsCurve, ProjectedCurve},
    },
    geom::{
        assert_array::{labels, AssertArray, ObjectKey},
        extent1d::Extent1d,
        extent2d::Extent2d,
        transform::Transform,
    },
    math::{Point2, Vec2, Vec3, UNDEF_DOUBLE},
    nurbs::b_spline_curve::p1::ProjectionType,
    surface::{
        plane::Plane,
        surface::Surface,
        volume::{Volume, VolumeKind},
    },
    topology::brep::validation::curve_assert_valid,
    Kernel,
};

const KEY: ObjectKey = ObjectKey(7);

/// Runs the check at `SM_LEVEL_0`; returns the result and the (reporting type,
/// test index, reporter is the checked curve) of each report.
fn check(c: Curve) -> (bool, Vec<(i64, u64, bool)>) {
    let k = Kernel::default();
    let mut c = c;
    let mut list = AssertArray::new();
    let ok = curve_assert_valid(&k, &mut c, Some(&mut list), 0, KEY).unwrap();
    let reports = list
        .reports()
        .iter()
        .map(|r| (r.reporting_type, r.test_index, r.reporter == Some(KEY)))
        .collect();
    (ok, reports)
}

fn line3(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::from(Line::from_points(
        Vec3::new(a[0], a[1], a[2]),
        Vec3::new(b[0], b[1], b[2]),
        3,
    ))
}

fn unit_plane() -> Surface {
    let domain = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &domain,
    ))
}

fn uv_line(a: [f64; 2], b: [f64; 2]) -> Curve {
    Curve::from(Line::from_points(
        Vec3::new(a[0], a[1], 0.0),
        Vec3::new(b[0], b[1], 0.0),
        2,
    ))
}

#[test]
fn hermite_curve() {
    let h = HermiteCurve::new(Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y, 3);
    assert_eq!(check(h.into()), (true, vec![]));

    // an undefined start point: test 0 fails, the power basis stays defined
    let h = HermiteCurve::from_2d(
        Point2::new(UNDEF_DOUBLE, 0.0),
        Vec2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
        2,
    );
    assert_eq!(
        check(h.into()),
        (false, vec![(types::HERMITE_CURVE_TYPE, 0, true)])
    );
}

#[test]
fn offset_curve() {
    let ivl = Extent1d::from_min_max(0.0, 1.0);
    let base = line3([0.0; 3], [1.0, 0.0, 0.0]);
    let owned = OffsetCurve::new(3, base, ivl, Vec3::Z, 0.5, true).unwrap();
    assert_eq!(check(owned.into()), (true, vec![]));

    // a borrowed base curve is not owned by the offset curve: test 1
    let base = line3([0.0; 3], [1.0, 0.0, 0.0]);
    let borrowed = OffsetCurve::new(3, base, ivl, Vec3::Z, 0.5, false).unwrap();
    assert_eq!(
        check(borrowed.into()),
        (false, vec![(labels::OFFSET_CURVE_TYPE, 1, true)])
    );
}

#[test]
fn projected_curve() {
    let base = line3([0.0, 0.0, 1.0], [1.0, 1.0, 2.0]);
    let p = ProjectedCurve::new(
        base,
        Some(Vec3::ZERO),
        Some(Vec3::Z),
        None,
        ProjectionType::Parallel,
        OwnsCurve::Yes,
    )
    .unwrap();
    assert_eq!(check(p.into()), (true, vec![]));

    // an unknown projection type: test 0 fails and the rest are skipped
    let base = line3([0.0, 0.0, 1.0], [1.0, 1.0, 2.0]);
    let p = ProjectedCurve::new(
        base,
        Some(Vec3::ZERO),
        Some(Vec3::Z),
        None,
        ProjectionType::Unknown,
        OwnsCurve::Yes,
    )
    .unwrap();
    assert_eq!(
        check(p.into()),
        (false, vec![(labels::PROJECTED_CURVE_TYPE, 0, true)])
    );
}

#[test]
fn crv_on_surf() {
    let k = Kernel::default();
    let c = CrvOnSurf::new(&k, uv_line([0.1, 0.2], [0.9, 0.7]), unit_plane(), None, 2).unwrap();
    assert_eq!(check(c.into()), (true, vec![]));

    // the UV curve leaves the surface's domain: test 2
    let c = CrvOnSurf::new(&k, uv_line([0.1, 0.2], [3.0, 0.7]), unit_plane(), None, 2).unwrap();
    assert_eq!(
        check(c.into()),
        (false, vec![(types::CRV_ON_SURF_TYPE, 2, true)])
    );

    // the UV-line constructor owns its line and (flag 2) the surface but never links
    // them to the curve (SetAll with flag 0): tests 3 and 4
    let c = CrvOnSurf::from_uv_line(
        &k,
        Point2::new(0.1, 0.2),
        Point2::new(0.9, 0.7),
        unit_plane(),
        None,
        2,
    )
    .unwrap();
    let t = types::CRV_ON_SURF_TYPE;
    assert_eq!(check(c.into()), (false, vec![(t, 3, true), (t, 4, true)]));
}

#[test]
fn crv_in_volume() {
    let volume = || Volume::new(VolumeKind::Transform(Transform::IDENTITY));
    let curve = || line3([0.0; 3], [1.0, 1.0, 1.0]);
    let owned = CrvInVolume::new(curve(), true, volume(), 3, 0).unwrap();
    assert_eq!(check(owned.into()), (true, vec![]));

    // neither part owned: the owner and context tests 3..6 fail
    let shared = CrvInVolume::new(curve(), true, volume(), 0, 0).unwrap();
    let c = types::CRV_IN_VOLUME_TYPE;
    assert_eq!(
        check(shared.into()),
        (
            false,
            vec![(c, 3, true), (c, 4, true), (c, 5, true), (c, 6, true)]
        )
    );
}
