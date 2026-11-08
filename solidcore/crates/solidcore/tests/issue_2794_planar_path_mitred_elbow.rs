//! #2794: a circle swept along two perpendicular lines (the shape of difftest
//! `primitive.primitive_creation_p2` `gen0092`/`gen0189`, 100 times larger: those have radius 1e-3,
//! legs 1e-2, and their body is not valid, see `plan/findings/`). The elbow's net is its
//! definition exactly and the mitre's union succeeds (the fitted net failed it with status 1001).
//! The result is checked directly: a valid closed body of the closed-form volume.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::{model::PrimitiveModel, p1::PrimitiveCreation};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::Kernel;

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
    let (radius, leg) = (1e-1, 1.);
    let mut k = Kernel::default();
    let mut b = elbow(&mut k, radius, leg, 1e-7);
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
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
    // the mitre plane passes through the bend's centre point, so the volume is the straight
    // pipe of the summed leg lengths
    let exact = std::f64::consts::PI * radius * radius * 2. * leg;
    assert!((v - exact).abs() < exact * 1e-4, "{v} != {exact}");
}
