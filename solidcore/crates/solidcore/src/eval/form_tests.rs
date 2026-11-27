//! The NURBS forms' tests: a whole turn's parameterization on its frame's quarter grid.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::*;

/// A whole turn starting a hair off a quarter line is parameterized exactly from that line (a
/// seam measured at coordinates tens of thousands of mm from the origin carries a few units of
/// the position's last place over the radius as its angle), while a seam really off the line by
/// more than that keeps its own start.
#[test]
fn a_whole_turn_a_hair_off_a_quarter_line_starts_on_it() {
    let far = 1.0 + 4.0e4 / 2.6;
    let hair = 64.0 * f64::EPSILON * far;
    let t = [1.9e-11, 1.9e-11 + TAU];
    assert!(t[0] > 0.0 && t[0] < hair, "the case's hair is within the room taken");
    assert_eq!(whole_turn_on_line(t, far), [0.0, TAU]);
    // The same whole turn the other side of the line, and a start genuinely off it.
    assert_eq!(whole_turn_on_line([-1.9e-11, -1.9e-11 + TAU], far), [0.0, TAU]);
    assert_eq!(whole_turn_on_line([1.0e-5, 1.0e-5 + TAU], far), [1.0e-5, 1.0e-5 + TAU]);
    // Less than a turn is never moved.
    assert_eq!(whole_turn_on_line([1.9e-11, 1.9e-11 + 1.0], far), [1.9e-11, 1.9e-11 + 1.0]);
}

/// The form of a whole-turn circle whose seam is a hair off a quarter line is four whole
/// quarters on the line: no control points a hair apart (issue #22886's check failure,
/// `SmBSplineCurve` test #15), and the edge's parameters run the form's whole range.
#[test]
fn a_whole_turn_circles_form_holds_no_hair_quarters() {
    let frame = Frame {
        origin: [4.0e4, 2.7e4, -1.8e4],
        x_axis: [0.95, -0.23, 0.21],
        y_axis: [-0.22, -0.06, 0.97],
    };
    let radius = 2.6;
    let t = [1.9e-11, 1.9e-11 + TAU];
    let (form, on) = curve(&Curve::Circle { frame: frame.clone(), radius }, t).unwrap();
    let Curve::RationalSpline { knots, homogeneous } = form else {
        panic!("a circle's form is a rational spline")
    };
    // The form's parameter counts quarter turns: the whole turn spans exactly four of them.
    assert!((on[0] - 0.0).abs() < 1e-9 && (on[1] - 4.0).abs() < 1e-9, "{on:?}");
    let least = homogeneous
        .windows(2)
        .map(|w| {
            let (a, b) = (
                [w[0][0] / w[0][3], w[0][1] / w[0][3], w[0][2] / w[0][3]],
                [w[1][0] / w[1][3], w[1][1] / w[1][3], w[1][2] / w[1][3]],
            );
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        })
        .fold(f64::MAX, f64::min);
    assert!(least > radius * 0.1, "control points {least:e} apart");
    // The knots are whole quarters: the form is not cut to the hair.
    assert!(knots.iter().all(|k| (k.round() - k).abs() < 1e-9), "{knots:?}");
}

/// A whole turn whose seam is really off the quarter line keeps its fifth quarter, cut to the
/// seam: #21017's stepped tapered hole closes on its seam that way.
#[test]
fn a_whole_turn_really_off_the_line_keeps_its_fifth_quarter() {
    let frame = Frame { origin: [0.0, 1.0, 0.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.0, 1.0] };
    let (form, _) = curve(&Curve::Circle { frame, radius: 2.6 }, [1.0e-5, 1.0e-5 + TAU]).unwrap();
    let Curve::RationalSpline { knots, .. } = form else {
        panic!("a circle's form is a rational spline")
    };
    let last = *knots.last().unwrap();
    let first = knots[0];
    assert!((last - first - 4.0).abs() < 1e-9, "{knots:?}");
    assert!(first > 0.0 && last > 4.0, "{knots:?}");
}
