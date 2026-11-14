//! A curve's CVs stretched from a pivot through ops: the pivot stays, the dragged CV moves by the
//! drag, a CV halfway along the control polygon by half of it, None moves every picked CV whole,
//! and a planar curve kept in its plane stays planar.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{stretch_cvs, Stretch};
use solidcore_ops::curve_plane::plane_of;
use solidcore_ops::Error;
use solidcore_record::brep::Curve;

/// A cubic of five CVs 2 apart along x in z = 5.
fn row() -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: (0..5).map(|i| [2.0 * f64::from(i), 0.0, 5.0]).collect(),
    }
}

fn cvs(c: &Curve) -> Vec<[f64; 3]> {
    let Curve::Spline { points, .. } = c else {
        panic!("{c:?}")
    };
    points.clone()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-12)
}

#[test]
fn the_pivot_stays_the_handle_moves_whole_and_halfway_moves_half() {
    let out = stretch_cvs(
        &row(),
        &[0, 1, 2, 3, 4],
        0,
        4,
        [0.0, 4.0, 0.0],
        Stretch::Linear,
        None,
    )
    .unwrap();
    let p = cvs(&out);
    assert!(near(p[0], [0.0, 0.0, 5.0]), "pivot: {p:?}");
    assert!(near(p[4], [8.0, 4.0, 5.0]), "handle: {p:?}");
    assert!(near(p[2], [4.0, 2.0, 5.0]), "halfway: {p:?}");
}

#[test]
fn without_falloff_every_picked_cv_moves_whole() {
    let out = stretch_cvs(
        &row(),
        &[1, 2, 3],
        0,
        4,
        [1.0, 1.0, 1.0],
        Stretch::None,
        None,
    )
    .unwrap();
    let p = cvs(&out);
    for i in 1..=3 {
        assert!(near(p[i], [2.0 * i as f64 + 1.0, 1.0, 6.0]), "{i}: {p:?}");
    }
    assert!(
        near(p[0], [0.0, 0.0, 5.0]) && near(p[4], [8.0, 0.0, 5.0]),
        "{p:?}"
    );
}

#[test]
fn kept_in_its_plane_a_planar_curve_stays_planar() {
    let wave = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [0.0, 0.0, 5.0],
            [2.0, 3.0, 5.0],
            [4.0, -1.0, 5.0],
            [6.0, 2.0, 5.0],
            [8.0, 0.0, 5.0],
        ],
    };
    let plane = plane_of(std::slice::from_ref(&wave)).unwrap().unwrap();
    let out = stretch_cvs(
        &wave,
        &[1, 2, 3, 4],
        0,
        4,
        [0.0, 2.0, 7.0],
        Stretch::Linear,
        Some(plane),
    )
    .unwrap();
    assert!(
        plane_of(std::slice::from_ref(&out)).unwrap().is_some(),
        "{out:?}"
    );
    assert!(
        cvs(&out).iter().all(|p| (p[2] - 5.0).abs() < 1e-9),
        "{out:?}"
    );
}

#[test]
fn bad_picks_are_refused() {
    let d = [0.0, 1.0, 0.0];
    assert_eq!(
        stretch_cvs(&row(), &[9], 0, 4, d, Stretch::Linear, None).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        stretch_cvs(&row(), &[1], 2, 2, d, Stretch::Linear, None).err(),
        Some(Error::Argument)
    );
    let line = Curve::Line {
        start: [0.0; 3],
        end: [1.0, 0.0, 0.0],
    };
    assert_eq!(
        stretch_cvs(&line, &[0], 0, 1, d, Stretch::None, None).err(),
        Some(Error::Argument)
    );
}
