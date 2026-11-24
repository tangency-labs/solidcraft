//! Curves dropped onto a face square to it, and a curve's record form: a line above a box's top
//! drops to its foot, a line beside a cylinder drops onto its wall at the radius, a curve over
//! nothing of the face is refused, and a rational quarter circle comes back as a rational
//! spline of homogeneous points.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::sweep::{drop_curves, Curve};
use solidcore_ops::{make_box, make_cylinder, Body, Error, Order};
use solidcore_record::brep::Curve as RecordCurve;

/// The record face of `body` whose area's centroid is at `z` (a box's top), or its one face
/// that is not flat (a cylinder's wall) when `z` is `None`.
fn face(body: &Body, order: &Order, z: Option<f64>) -> usize {
    (0..order.faces.len())
        .find(|&f| match z {
            Some(z) => (body.face_centroid(order, f, 1e-9).unwrap().centroid[2] - z).abs() < 1e-6,
            None => body.face_plane_normal(order, f).unwrap().is_none(),
        })
        .unwrap()
}

/// The control points of a dropped NURBS, made Euclidean.
fn points(c: &Curve) -> Vec<[f64; 3]> {
    let Curve::Nurbs { points, .. } = c else {
        panic!("not a NURBS: {c:?}")
    };
    points
        .iter()
        .map(|p| [p[0] / p[3], p[1] / p[3], p[2] / p[3]])
        .collect()
}

#[test]
fn a_line_above_a_box_drops_to_its_foot_on_the_top() {
    let cube = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = cube.to_record().unwrap();
    let top = face(&cube, &order, Some(10.0));
    let line = Curve::Line {
        start: [2.0, 5.0, 15.0],
        end: [8.0, 5.0, 15.0],
    };
    let dropped = drop_curves(&cube, &order, top, &[line], 1e-6).unwrap();
    assert_eq!(dropped.len(), 1);
    let p = points(&dropped[0].0);
    // the drop may run either way along the line: each end lands on one of the curve's ends
    let ends = [p[0], p[p.len() - 1]];
    for want in [[2.0, 5.0, 10.0], [8.0, 5.0, 10.0]] {
        assert!(
            ends.iter()
                .any(|e| (0..3).all(|k| (e[k] - want[k]).abs() < 1e-6)),
            "{want:?} in {p:?}"
        );
    }
    for q in &p {
        assert!((q[2] - 10.0).abs() < 1e-6, "on the top: {q:?}");
    }
}

#[test]
fn a_line_beside_a_cylinder_drops_onto_its_wall_at_the_radius() {
    let can = make_cylinder([0.0; 3], 5.0, 20.0).unwrap();
    let (_, order) = can.to_record().unwrap();
    let wall = face(&can, &order, None);
    let line = Curve::Line {
        start: [8.0, 0.0, 2.0],
        end: [8.0, 0.0, 18.0],
    };
    let dropped = drop_curves(&can, &order, wall, &[line], 1e-6).unwrap();
    assert!(!dropped.is_empty());
    for (c, _) in &dropped {
        for q in points(c) {
            let r2 = q[0] * q[0] + q[1] * q[1];
            assert!((r2 - 25.0).abs() < 1e-5, "at the radius: {q:?}");
            assert!(
                q[1].abs() < 1e-6,
                "square to the wall, in the line's plane: {q:?}"
            );
        }
    }
}

#[test]
fn a_curve_over_nothing_of_the_face_is_refused() {
    let cube = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = cube.to_record().unwrap();
    let top = face(&cube, &order, Some(10.0));
    let far = Curve::Line {
        start: [50.0, 5.0, 15.0],
        end: [60.0, 5.0, 15.0],
    };
    assert_eq!(
        drop_curves(&cube, &order, top, &[far], 1e-6).err(),
        Some(Error::Empty)
    );
    let line = Curve::Line {
        start: [2.0, 5.0, 15.0],
        end: [8.0, 5.0, 15.0],
    };
    assert_eq!(
        drop_curves(&cube, &order, 99, &[line], 1e-6).err(),
        Some(Error::Argument)
    );
}

/// A quarter circle of radius 10 as a rational quadratic: its record is the rational spline of
/// the same knots and homogeneous points (each point times its weight).
#[test]
fn a_rational_quarter_circle_comes_back_as_a_rational_spline() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let arc = Curve::Nurbs {
        degree: 2,
        points: vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0, 10.0, 0.0, w],
            [0.0, 10.0, 0.0, 1.0],
        ],
        knots: knots.clone(),
    };
    let r = arc.to_record().unwrap();
    let RecordCurve::RationalSpline {
        knots: got,
        homogeneous,
    } = &r.curve
    else {
        panic!("not a rational spline: {r:?}")
    };
    assert_eq!(got, &knots);
    let want = [
        [10.0, 0.0, 0.0, 1.0],
        [10.0 * w, 10.0 * w, 0.0, w],
        [0.0, 10.0, 0.0, 1.0],
    ];
    for (g, w) in homogeneous.iter().zip(want) {
        assert!(
            (0..4).all(|k| (g[k] - w[k]).abs() < 1e-12),
            "{homogeneous:?}"
        );
    }
    assert_eq!(r.interval, [0.0, 1.0]);
    assert!(r.nurbs.is_none());
    // a plain line is a line
    let line = Curve::Line {
        start: [0.0; 3],
        end: [1.0, 2.0, 3.0],
    };
    assert!(matches!(
        line.to_record().unwrap().curve,
        RecordCurve::Line { .. }
    ));
}
