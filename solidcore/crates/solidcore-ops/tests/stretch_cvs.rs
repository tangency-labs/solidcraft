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

fn periodic_square() -> Curve {
    let square = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    let mut points = square.to_vec();
    points.extend_from_slice(&square[..3]);
    Curve::Spline {
        knots: (0..=10).map(f64::from).collect(),
        points,
    }
}

#[test]
fn periodic_stretch_is_independent_of_pick_order_and_repeated_slots() {
    let curve = periodic_square();
    let mut expected = curve.clone();
    let Curve::Spline { points, .. } = &mut expected else {
        panic!("a spline")
    };
    points[0][2] = 4.0;
    points[4][2] = 4.0;
    for picked in [&[0, 4][..], &[4, 0], &[0], &[4], &[4, 0, 4, 0]] {
        assert_eq!(
            stretch_cvs(&curve, picked, 1, 2, [0.0, 0.0, 4.0], Stretch::Linear, None).unwrap(),
            expected,
            "{picked:?}"
        );
    }
}

#[test]
fn periodic_pivot_and_handle_use_the_same_point_as_their_copies() {
    let curve = periodic_square();
    let mut expected = curve.clone();
    let Curve::Spline { points, .. } = &mut expected else {
        panic!("a spline")
    };
    for i in [1, 5] {
        points[i][2] = 2.0;
    }
    for i in [2, 6] {
        points[i][2] = 4.0;
    }
    for pivot in [0, 4] {
        for handle in [2, 6] {
            for picked in [&[0, 1, 2][..], &[6, 5, 4], &[0, 4, 1, 5, 2, 6]] {
                assert_eq!(
                    stretch_cvs(
                        &curve,
                        picked,
                        pivot,
                        handle,
                        [0.0, 0.0, 4.0],
                        Stretch::Linear,
                        None
                    )
                    .unwrap(),
                    expected,
                    "{pivot}, {handle}, {picked:?}"
                );
            }
        }
    }
}

#[test]
fn periodic_stretch_refuses_an_aliased_handle_or_out_of_range_pick() {
    let curve = periodic_square();
    for how in [Stretch::Linear, Stretch::None] {
        for (pivot, handle) in [(0, 4), (4, 0), (1, 5), (2, 6), (7, 2), (0, 7)] {
            assert_eq!(
                stretch_cvs(&curve, &[0], pivot, handle, [0.0, 0.0, 4.0], how, None),
                Err(Error::Argument)
            );
        }
        for picked in [vec![7], vec![usize::MAX]] {
            assert_eq!(
                stretch_cvs(&curve, &picked, 0, 2, [0.0, 0.0, 4.0], how, None),
                Err(Error::Argument)
            );
        }
    }
}

#[test]
fn periodic_stretch_keeps_plane_projection_and_uniform_drags() {
    let curve = periodic_square();
    let plane = plane_of(std::slice::from_ref(&curve)).unwrap().unwrap();
    for (how, moved) in [
        (Stretch::None, [2.0, 0.0, 0.0]),
        (Stretch::Linear, [0.0, 0.0, 0.0]),
    ] {
        let out = stretch_cvs(&curve, &[4, 0, 4], 0, 2, [2.0, 0.0, 7.0], how, Some(plane)).unwrap();
        let mut expected = curve.clone();
        let Curve::Spline { points, .. } = &mut expected else {
            panic!("a spline")
        };
        points[0] = moved;
        points[4] = moved;
        assert_eq!(out, expected);
    }
}

#[test]
fn periodic_stretch_moves_every_copy_when_a_point_has_more_than_one() {
    let a = [0.0, 0.0, 0.0];
    let b = [10.0, 0.0, 0.0];
    let curve = Curve::Spline {
        knots: (0..=8).map(f64::from).collect(),
        points: vec![a, b, a, b, a],
    };
    let expected = Curve::Spline {
        knots: (0..=8).map(f64::from).collect(),
        points: vec![a, [10.0, 0.0, 4.0], a, [10.0, 0.0, 4.0], a],
    };
    for pivot in [0, 2, 4] {
        for handle in [1, 3] {
            assert_eq!(
                stretch_cvs(
                    &curve,
                    &[4, 3, 2, 1, 0],
                    pivot,
                    handle,
                    [0.0, 0.0, 4.0],
                    Stretch::Linear,
                    None
                )
                .unwrap(),
                expected
            );
        }
    }
    let moved = stretch_cvs(&curve, &[4], 1, 2, [0.0, 0.0, 4.0], Stretch::Linear, None).unwrap();
    assert_eq!(
        moved,
        Curve::Spline {
            knots: (0..=8).map(f64::from).collect(),
            points: vec![[0.0, 0.0, 4.0], b, [0.0, 0.0, 4.0], b, [0.0, 0.0, 4.0]],
        }
    );
}

#[test]
fn coincident_points_on_nonperiodic_curves_stretch_independently() {
    let a = [0.0, 0.0, 0.0];
    let b = [10.0, 0.0, 0.0];
    let points = vec![a, b, a];
    for curve in [
        Curve::Polyline {
            points: points.clone(),
        },
        Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            points,
        },
    ] {
        let out = stretch_cvs(&curve, &[2], 0, 1, [0.0, 0.0, 4.0], Stretch::Linear, None).unwrap();
        let mut expected = curve.clone();
        match &mut expected {
            Curve::Spline { points, .. } | Curve::Polyline { points } => points[2][2] = 8.0,
            _ => panic!("control points"),
        }
        assert_eq!(out, expected);
    }
}
