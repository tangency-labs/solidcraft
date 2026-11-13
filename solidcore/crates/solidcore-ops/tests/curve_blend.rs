//! The blend from one curve's end to another's (`curve::blend_curve`) on closed forms: two lines
//! joined at G0, G1 and G2 (the degree, the ends, the inner points on the tangents, no curvature
//! where a line is met at G2), a quarter circle met at G2 (the blend's curvature at its start is
//! the circle's 1/10, towards the centre), and the refusals.
// The closed forms use square roots; the tests compute them, not ops.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use solidcore_ops::curve::{blend_curve, curve_comb, Contact, End};
use solidcore_ops::Error;
use solidcore_record::brep::Curve;

fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![p, q],
    }
}

/// The spline's control points.
fn points(c: &Curve) -> &[[f64; 3]] {
    let Curve::Spline { points, .. } = c else {
        panic!("not a spline: {c:?}");
    };
    points
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The first line ends at (10, 0, 0) along +x, the second starts at (20, 5, 0) along +x. At G0,
/// G1 and G2 the blend is of degree 1, 3 and 5 from end to end; at G1 and G2 its inner points
/// lie on each line's tangent, 0.3 of the chord from the end; at G2 its curvature is 0 at both
/// ends, as the lines'.
#[test]
fn two_lines_at_g0_g1_and_g2() {
    let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let b = line([20.0, 5.0, 0.0], [30.0, 5.0, 0.0]);
    let chord = length([10.0, 5.0, 0.0]);
    for (contact, order) in [(Contact::G0, 0), (Contact::G1, 1), (Contact::G2, 2)] {
        let blend = blend_curve(&a, End::End, contact, 0.3, &b, End::Start, contact, 0.3).unwrap();
        let p = points(&blend);
        let n = p.len() - 1;
        assert_eq!(n, 2 * order + 1, "{contact:?}");
        assert!(near(p[0], [10.0, 0.0, 0.0]) && near(p[n], [20.0, 5.0, 0.0]));
        if order >= 1 {
            assert!(near(p[1], [10.0 + 0.3 * chord, 0.0, 0.0]), "{:?}", p[1]);
            assert!(
                near(p[n - 1], [20.0 - 0.3 * chord, 5.0, 0.0]),
                "{:?}",
                p[n - 1]
            );
        }
        if order == 2 {
            let comb = curve_comb(&blend, [0.0, 1.0], 5).unwrap();
            let (first, last) = (&comb[0], &comb[comb.len() - 1]);
            assert!(near(first.at, [10.0, 0.0, 0.0]) && near(last.at, [20.0, 5.0, 0.0]));
            assert!(length(first.curvature) < 1e-9 && length(last.curvature) < 1e-9);
        }
    }
}

/// A quarter circle of radius 10 about the origin, ending at (0, 10, 0) along -x, met at G2,
/// and a line starting at (-10, 14, 0) along -x met at G1: the blend is a quartic whose
/// curvature at its start is 1/10, pointing to the circle's centre (-y).
#[test]
fn a_circle_arc_met_at_g2() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0 * w, 10.0 * w, 0.0, w],
            [0.0, 10.0, 0.0, 1.0],
        ],
    };
    let b = line([-10.0, 14.0, 0.0], [-20.0, 14.0, 0.0]);
    let blend = blend_curve(
        &arc,
        End::End,
        Contact::G2,
        0.4,
        &b,
        End::Start,
        Contact::G1,
        0.4,
    )
    .unwrap();
    assert_eq!(points(&blend).len(), 5, "a quartic");
    let comb = curve_comb(&blend, [0.0, 1.0], 5).unwrap();
    let first = &comb[0];
    assert!(near(first.at, [0.0, 10.0, 0.0]), "{:?}", first.at);
    let kappa = length(first.curvature);
    assert!((kappa - 0.1).abs() < 1e-9, "{kappa}");
    assert!(first.curvature[1] < 0.0, "towards the centre");
}

#[test]
fn bad_asks_are_refused() {
    let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let b = line([20.0, 5.0, 0.0], [30.0, 5.0, 0.0]);
    let g1 = Contact::G1;
    // the second curve starts where the first ends
    let touching = line([10.0, 0.0, 0.0], [20.0, 5.0, 0.0]);
    assert_eq!(
        blend_curve(&a, End::End, g1, 0.3, &touching, End::Start, g1, 0.3),
        Err(Error::Argument),
        "coincident ends"
    );
    // a quadratic whose first two control points coincide: no tangent at its start
    let stub = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
    };
    assert_eq!(
        blend_curve(&stub, End::Start, g1, 0.3, &b, End::Start, g1, 0.3),
        Err(Error::Argument),
        "a zero-length tangent"
    );
    for shape in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            blend_curve(&a, End::End, g1, shape, &b, End::Start, g1, 0.3),
            Err(Error::Argument),
            "shape {shape}"
        );
    }
    let unbounded = Curve::Line {
        start: [0.0, 0.0, 0.0],
        end: [1.0, 0.0, 0.0],
    };
    assert_eq!(
        blend_curve(&unbounded, End::End, g1, 0.3, &b, End::Start, g1, 0.3),
        Err(Error::Argument),
        "a line has no end"
    );
}
