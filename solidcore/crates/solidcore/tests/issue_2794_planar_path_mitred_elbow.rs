//! #2794, #13687: a circle swept along two perpendicular lines (the shape of difftest
//! `primitive.primitive_creation_p2` `gen0092`/`gen0189`, which have radius 1e-3 and legs
//! 1e-2). The elbow's net is its definition exactly and the mitre's union succeeds (the
//! fitted net failed it with status 1001). The mitre trim names the side to delete by a
//! point that stays within the reach of its cutting line, so a small profile is trimmed
//! like a large one. The result is checked directly: a valid closed body of the
//! closed-form volume.
//!
//! #17314, WR-793: a corner too sharp for its mitre. The pieces run past a corner by
//! `2 sqrt(2) M` (`M` the largest coordinate of the profile's box), and the cutting line
//! reaches as far from it. The mitre of a profile `w` wide on either side of the path
//! reaches `w / sin(beta / 2)` from the corner, `beta` the angle between the legs; past
//! the reach the pieces stay open and the sweep came back as a body without volume.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::curve::curve::Curve;
use solidcore::error::primitive::PrimitiveFailure;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::{model::PrimitiveModel, p1::PrimitiveCreation};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, SolidError};

fn elbow(k: &mut Kernel, radius: f64, leg: f64, tol: f64) -> solidcore::Brep {
    let profile = Curve::from(
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y),
            radius,
            0.,
            360.,
            NurbCircleParam::Quadratic,
        )
        .unwrap(),
    );
    let up = Vec3::new(0., 0., leg);
    let path = |a, b| Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap());
    let paths = [path(Vec3::ZERO, up), path(up, Vec3::new(leg, 0., leg))];
    let (_, mut b) = PrimitiveModel::build_new(k, |_, model| {
        let id = PrimitiveCreation::create_sweep_along_planar_path(
            model,
            PrimitiveModel::MAIN,
            &[profile],
            &paths,
            tol,
            true,
            true,
            true,
        )?;
        model.take_brep(id)
    })
    .unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
fn elbow_of_two_lines_is_a_valid_body_of_the_closed_form_volume() {
    // the first is the size that always worked; the others returned an invalid body or
    // none (#13687), the last is `gen0092`/`gen0189`
    for (radius, leg, tol) in [
        (1e-1, 1., 1e-7),
        (1.85e-2, 0.5, 1e-7),
        (1e-2, 1e-1, 1e-5),
        (1e-3, 1., 1e-7),
        (1e-3, 1e-2, 1e-7),
    ] {
        let mut k = Kernel::default();
        let mut b = elbow(&mut k, radius, leg, tol);
        // two caps and the two legs' walls: the mitre removed all of each wall beyond it
        assert_eq!(b.face_list().len(), 4, "radius {radius} leg {leg}");
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "radius {radius} leg {leg}: {:?}",
            check.lines()
        );
        let store = AttributeStore::default();
        let v = brep_volume(
            &k,
            &mut b,
            1e-12,
            &PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            },
        )
        .unwrap();
        // the mitre plane passes through the bend's centre point, so the volume is the
        // straight pipe of the summed leg lengths
        let exact = std::f64::consts::PI * radius * radius * 2. * leg;
        assert!(
            (v - exact).abs() < exact * 1e-4,
            "radius {radius} leg {leg}: {v} != {exact}"
        );
    }
}

/// A circle of radius 1 centred at `(0, centre_y, 0)`, moved onto the path
/// `(0,0,0) -> (10,0,0) -> back at a_deg` (two legs of 10), `tol = 1e-3`, capped.
fn turn(k: &mut Kernel, a_deg: f64, centre_y: f64) -> solidcore::Result<solidcore::Brep> {
    let profile = Curve::from(
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(Vec3::new(0., centre_y, 0.), Vec3::Y, Vec3::Z),
            1.,
            0.,
            360.,
            NurbCircleParam::Quadratic,
        )
        .unwrap(),
    );
    let a = a_deg.to_radians();
    let corner = Vec3::new(10., 0., 0.);
    let end = Vec3::new(10. - 10. * a.cos(), 10. * a.sin(), 0.);
    let path = |p, q| Curve::from(BSplineCurve::create_line_segment(3, p, q).unwrap());
    let paths = [path(Vec3::ZERO, corner), path(corner, end)];
    PrimitiveModel::build_new(k, |_, model| {
        let id = PrimitiveCreation::create_sweep_along_planar_path(
            model,
            PrimitiveModel::MAIN,
            &[profile],
            &paths,
            1e-3,
            true,
            true,
            true,
        )?;
        model.take_brep(id)
    })
    .map(|(_, b)| b)
}

#[test]
fn a_corner_too_sharp_for_its_mitre_is_refused() {
    // A path that turns back within 2 degrees was taken for a smooth one and never
    // mitred: two overlapping tubes, open at the corner, came back with 16 faces.
    for a in [0.5, 1., 1.2] {
        let e = turn(&mut Kernel::default(), a, 0.).err();
        assert_eq!(
            e.as_ref().and_then(SolidError::primitive_failure),
            Some(&PrimitiveFailure::PathTurnsBack),
            "a {a}: {e:?}"
        );
    }
    // With the circle at the origin `M = 1`, so the reach is `2 sqrt(2)` and the mitre
    // fits from beta = 2 asin(1 / (2 sqrt(2))) = 41.4 degrees. Below it the pieces stay
    // open (22 faces at 2.5 degrees, the two caps alone from 12 to 41).
    for a in [2.5, 10., 30., 41.] {
        let e = turn(&mut Kernel::default(), a, 0.).err();
        assert_eq!(
            e.as_ref().and_then(SolidError::primitive_failure),
            Some(&PrimitiveFailure::SweepDoesNotClose),
            "a {a}: {e:?}"
        );
    }
    // The same circle drawn at y = 3 has `M = 4`: the reach is four times longer and a
    // 12 degree corner fits (from 10.1 degrees), as does 42 degrees at the origin. The
    // legs are long enough that the mitre plane leaves each tube through its wall, so the
    // volume is the straight pipe of the summed leg lengths.
    for (a, centre_y) in [(12., 3.), (42., 0.)] {
        let mut k = Kernel::default();
        let mut b = turn(&mut k, a, centre_y).unwrap();
        b.refresh_cached_topology();
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (4, 5, 3),
            "a {a}"
        );
        assert!(
            solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap(),
            "a {a}"
        );
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "a {a}: {:?}", check.lines());
        let store = AttributeStore::default();
        let v = brep_volume(
            &k,
            &mut b,
            1e-12,
            &PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            },
        )
        .unwrap();
        let exact = std::f64::consts::PI * 20.;
        assert!((v - exact).abs() < exact * 1e-4, "a {a}: {v} != {exact}");
    }
}

/// The corner where a closed path meets its start is checked as the others are: a thin
/// triangle `(0,0,0) -> (10,0,0) -> (10,h,0)` and back swept by a circle of radius
/// 0.01. At `h = 0.1` the path closes 0.57 degrees from turning back; at `h = 10` the
/// closing corner is 45 degrees, past the mitre's 41.4, and the ring closes.
#[test]
fn a_closed_path_turning_back_where_it_closes_is_refused() {
    let ring = |h: f64| {
        let mut k = Kernel::default();
        let profile = Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::new(Vec3::ZERO, Vec3::Y, Vec3::Z),
                0.01,
                0.,
                360.,
                NurbCircleParam::Quadratic,
            )
            .unwrap(),
        );
        let p = [Vec3::ZERO, Vec3::new(10., 0., 0.), Vec3::new(10., h, 0.)];
        let path = |p, q| Curve::from(BSplineCurve::create_line_segment(3, p, q).unwrap());
        let paths = [path(p[0], p[1]), path(p[1], p[2]), path(p[2], p[0])];
        PrimitiveModel::build_new(&mut k, |_, model| {
            let id = PrimitiveCreation::create_sweep_along_planar_path(
                model,
                PrimitiveModel::MAIN,
                &[profile],
                &paths,
                1e-3,
                false,
                false,
                true,
            )?;
            model.take_brep(id)
        })
        .map(|(_, b)| b)
    };
    let e = ring(0.1).err();
    assert_eq!(
        e.as_ref().and_then(SolidError::primitive_failure),
        Some(&PrimitiveFailure::PathTurnsBack),
        "{e:?}"
    );
    let mut b = ring(10.).unwrap();
    b.refresh_cached_topology();
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (3, 6, 3)
    );
    assert!(solidcore::topology::brep::p2::is_closed_manifold(&b).unwrap());
}
