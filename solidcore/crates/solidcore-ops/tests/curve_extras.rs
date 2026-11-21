//! An arc about a centre and a curve extended keeping its control points' proportions, through
//! ops on record curves.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
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

/// The length of the polyline through `c` at `n` even steps of each quarter of its parameter
/// (a comb has at most 2000 teeth): below the curve's arc length (an inscribed polygon), and
/// within the polyline's chord error of it.
fn inscribed_length(c: &Curve, n: usize) -> f64 {
    let mut total = 0.0;
    for q in 0..4 {
        let range = [f64::from(q) / 4.0, f64::from(q + 1) / 4.0];
        let pts: Vec<[f64; 3]> = curve_comb(c, range, n)
            .unwrap()
            .into_iter()
            .map(|t| t.at)
            .collect();
        total += pts
            .windows(2)
            .map(|w| {
                let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .sum::<f64>();
    }
    total
}

/// #16635: a hooked cubic (its polygon turns back at its end), length 9.848846342765029, grown
/// by 0.5 and 3 keeping its proportions. On main both came back Ok short of the length asked
/// (8.77 and 7.53, shorter than the curve). The inscribed polyline at 4 x 1000 stations is below
/// the arc length, which is within 1e-7 of the length asked (relative), so it is at most that;
/// and it is within its chord error (well under 1e-3 here) of it.
#[test]
fn a_hooked_cubic_extended_keeping_proportion_has_the_length_asked() {
    let c = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 0.1, 0.0],
            [9.0, 0.1, 0.0],
        ],
    };
    let before = 9.848_846_342_765_029;
    assert!((inscribed_length(&c, 1000) - before).abs() < 1e-3);
    for dist in [0.5, 3.0] {
        let out = extend_curve(&c, CurveEndAt::End, dist, CurveExtension::Proportion)
            .unwrap_or_else(|e| panic!("{dist}: {e:?}"));
        let target = before + dist;
        let got = inscribed_length(&out, 1000);
        assert!(
            got <= target * (1.0 + 1e-7) && got >= target - 1e-3,
            "{dist}: inscribed length {got}, asked {target}"
        );
    }
}
