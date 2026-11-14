//! An arc about a centre and a curve extended keeping its control points' proportions, through
//! ops on record curves.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{arc_about, curve_comb, extend_curve, CurveEndAt, CurveExtension};
use solidcore_ops::Error;
use solidcore_record::brep::Curve;

/// The points of `c` at 33 even steps of its parameter.
fn along(c: &Curve) -> Vec<[f64; 3]> {
    curve_comb(c, [0.0, 1.0], 33)
        .unwrap()
        .into_iter()
        .map(|t| t.at)
        .collect()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

/// About +Z from (10, 0, 0) to the +Y direction: a quarter of the circle of radius 10, from the
/// start to (0, 10, 0); about -Z the same picks run three quarters round, through the lower half.
#[test]
fn an_arc_about_a_centre_runs_anticlockwise_about_its_normal() {
    let quarter = arc_about([0.0; 3], [0.0, 0.0, 1.0], [10.0, 0.0, 0.0], [0.0, 5.0, 0.0]).unwrap();
    let p = along(&quarter);
    assert!(
        near(p[0], [10.0, 0.0, 0.0]) && near(p[32], [0.0, 10.0, 0.0]),
        "{:?} {:?}",
        p[0],
        p[32]
    );
    assert!(
        p.iter()
            .all(|q| (q[0] * q[0] + q[1] * q[1] - 100.0).abs() < 1e-6
                && q[0] >= -1e-9
                && q[1] >= -1e-9),
        "{p:?}"
    );
    let three = arc_about(
        [0.0; 3],
        [0.0, 0.0, -1.0],
        [10.0, 0.0, 0.0],
        [0.0, 5.0, 0.0],
    )
    .unwrap();
    let p = along(&three);
    assert!(
        near(p[0], [10.0, 0.0, 0.0]) && near(p[32], [0.0, 10.0, 0.0]),
        "{:?} {:?}",
        p[0],
        p[32]
    );
    assert!(
        p.iter().any(|q| q[1] < -9.0),
        "through the lower half: {p:?}"
    );
    assert!(p
        .iter()
        .all(|q| (q[0] * q[0] + q[1] * q[1] - 100.0).abs() < 1e-6));
}

#[test]
fn an_arc_with_its_start_or_end_on_the_axis_is_refused() {
    let z = [0.0, 0.0, 1.0];
    assert_eq!(
        arc_about([0.0; 3], z, [0.0, 0.0, 5.0], [1.0, 0.0, 0.0]).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        arc_about([0.0; 3], z, [1.0, 0.0, 0.0], [0.0; 3]).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        arc_about([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).err(),
        Some(Error::Argument)
    );
}

/// A straight cubic with its CVs at x = 0, 1, 3 and 6, extended 3 at its end keeping
/// proportion: its CVs at 0, 1.5, 4.5 and 9, the knots kept.
#[test]
fn a_proportional_extension_stretches_the_cvs_along_the_curve() {
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let c = Curve::Spline {
        knots: knots.clone(),
        points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [6.0, 0.0, 0.0],
        ],
    };
    let out = extend_curve(&c, CurveEndAt::End, 3.0, CurveExtension::Proportion).unwrap();
    let Curve::Spline { knots: k, points } = &out else {
        panic!("{out:?}")
    };
    assert_eq!(k, &knots);
    for (p, x) in points.iter().zip([0.0, 1.5, 4.5, 9.0]) {
        assert!(
            (p[0] - x).abs() < 1e-6 && p[1].abs() < 1e-12 && p[2].abs() < 1e-12,
            "{points:?}"
        );
    }
    assert_eq!(
        extend_curve(&c, CurveEndAt::End, -1.0, CurveExtension::Proportion).err(),
        Some(Error::Argument)
    );
}
