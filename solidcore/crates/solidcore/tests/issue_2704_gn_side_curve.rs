//! #2704: with `fillet_gn_section_generator` alone, a variable box round failed at one end
//! ("failure to calculate intersection of fillet surface with side face", 1001) for laws such
//! as 4 to 0, 2 to 0, 4 to 2, 0.5 to 4 and 8 to 4, and not for 4 to 3 or 0 to 4. The touch
//! circle's section leaves the end face's plane at an end, so the side curve must come from
//! the plane's equation (WR-682), not the plane's domain or the surfaces' domains.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::p1::{FilletLaw, FilletLawKind};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

/// The 10 x 20 x 30 box's edge at (10, 10, 30) rounded by the law from `from` at (10, 0, 30)
/// to `to` at (10, 20, 30): the volume it removes.
fn removed(from: f64, to: f64) -> f64 {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let e = pick(&k, &b, Point3::new(10.0, 10.0, 30.0));
    let ivl = edge::interval(&b, e).unwrap();
    let c = edge::curve(&b, e).unwrap().unwrap();
    let lo = c.evaluate(&k, ivl.min(), 0, true, false).unwrap()[0];
    let hi = c.evaluate(&k, ivl.max(), 0, true, false).unwrap()[0];
    let at = Point3::new(10.0, 0.0, 30.0);
    let (a, z) = if hi.distance(at) < lo.distance(at) {
        (to, from)
    } else {
        (from, to)
    };
    let law_curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &[Vec3::new(a, 0.0, 0.0), Vec3::new(z, 1.0, 0.0)],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let law = FilletLaw::new(FilletLawKind::BSpline {
        law_curve,
        extended_law: None,
    });
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_variable_radius_edges(&mut k, &[e], from.max(to), law, 1, 1)
            .unwrap();
        s.do_fillet(&mut k)
            .unwrap_or_else(|e| panic!("{from} to {to}: {e:?}: {}", s.error_info().comment));
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(
        check.lines().is_empty(),
        "{from} to {to}: {:?}",
        check.lines()
    );
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    6000.0 - brep_volume(&k, &mut b, 1e-7, &attrs).unwrap()
}

/// The issue's law, both ways round: one body, 22.596105 off (the union of its balls).
#[test]
fn a_round_from_4_to_0_removes_what_0_to_4_removes() {
    let (down, up) = (removed(4.0, 0.0), removed(0.0, 4.0));
    assert!((down - 22.596105).abs() <= 1e-4, "{down}");
    assert!((down - up).abs() <= 1e-4, "{down} vs {up}");
}

/// The other laws that failed, each the same both ways round.
#[test]
fn laws_that_failed_build_both_ways_round() {
    for (a, b) in [(4.0, 2.0), (2.0, 0.0), (8.0, 4.0), (4.0, 0.5)] {
        let (down, up) = (removed(a, b), removed(b, a));
        assert!((down - up).abs() <= 1e-4, "{a} {b}: {down} vs {up}");
    }
}
