//! A B-spline radius law round on a cube edge (difftest
//! `law-cube20-e0-deg1-4-2-4-c0-corner`, `prog_test` `smFilletTest.cpp:399`).
//!
//! `SmFilletEdge::CalcFilletIntSideFace` and `CalcCrossSection` work on upstream's
//! pointers to the faces' surfaces, which keep their face as owner; the port's copies
//! (`exact_surface`) keep it too. `sm_SurfaceDropPoint` then drops points onto such a
//! face-owned surface with point testing off (`m_bPointTestEnabled = FALSE`), so it
//! never classifies on the face. Before the solve switched the test off, the round
//! stopped at `Unported(SmFace::PointClassify, query.face_p2)`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::Kernel;

#[test]
fn bspline_law_round_drops_points_on_face_owned_surfaces_without_point_test() {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let edge = *p1::edges(&body, None).unwrap().first().unwrap();

    // degree 1, radii 4 -> 2 -> 4 over knots [0, 0.5, 1] (multiplicities 2, 1, 2)
    let control = [
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 0.0),
    ];
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[2, 1, 2],
        knots: &[0.0, 0.5, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    let law = FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0));

    let faces_before = p1::faces(&body, None).unwrap().len();
    {
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[edge], 4.0, law, 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    // the round adds its face
    assert_eq!(p1::faces(&body, None).unwrap().len(), faces_before + 1);
}
