//! #2727: great circles have no corner of a radius law to stop at (the 20 cube's edge rounded
//! by the linear law 4, 2, 4 built a net that doubles back on itself, `check_brep` #5, and
//! 3, 2, 4, 2, 3 one over 140 mm across). Every kernel now builds a variable round as the union
//! of its balls (WR-696; the flag `fillet_gn_section_generator` and the refusal of the great
//! circles over a law corner are retired with it): each side of a corner is its own piece.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1::edges;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The linear law through `r` at equal steps along the edge.
fn law(r: &[f64]) -> FilletLaw {
    let n = r.len();
    let knots: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
    let control: Vec<Vec3> = r.iter().map(|&r| Vec3::new(r, 0.0, 0.0)).collect();
    let mut mult = vec![1_usize; n];
    mult[0] = 2;
    mult[n - 1] = 2;
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &mult,
        knots: &knots,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

fn round(k: &mut Kernel, b: &mut Brep, r: &[f64]) -> Result<(), String> {
    let e = edges(b, None).unwrap()[0];
    let mut session = FilletSession::new(k, b).unwrap();
    session
        .prepare_variable_radius_edges(k, &[e], r[0], law(r), 1, 1)
        .unwrap();
    session
        .do_fillet(k)
        .map_err(|_| session.error_info().comment.clone())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// With the balls' union each side of a corner is its own piece: a sound body with less than
/// the cube's volume and area (the law peaks at 4, so at most the constant round 4 goes:
/// 20 x (1 - pi/4) x 16 = 68.7).
#[test]
fn the_union_of_the_balls_builds_them() {
    for r in [&[4.0, 2.0, 4.0][..], &[3.0, 2.0, 4.0, 2.0, 3.0]] {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        round(&mut k, &mut b, r).unwrap();
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.lines().is_empty(), "{r:?}: {:?}", check.lines());
        let v = volume(&k, &mut b);
        assert!(v < 8000.0 && v > 8000.0 - 68.7, "{r:?}: volume {v}");
        let area = brep_area(&k, &mut b, 1e-7).unwrap();
        assert!(area < 2400.0 && area > 2300.0, "{r:?}: area {area}");
    }
}
