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

/// Issue #20836: dropping curves across a closed cylinder wall's seam (the ruling at
/// x = 10, y = 0) produces two pieces meeting on the seam — both when a curve's knot
/// falls on the seam (the slot's semicircular ends) and when the crossing lies strictly
/// inside a single span (a straight horizontal line or an offset arc). Cutting the
/// closed cylinder wall with the dropped slot gives the closed-form area
/// `32 R atan(a / d) + 2 pi R (sqrt(a^2 + d^2) - d)`.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form test oracle, not ops computing geometry
fn curves_crossing_a_closed_cylinders_seam_drop_as_two_pieces_and_cut() {
    use solidcore_ops::trim::split_face_by_curves;
    use std::f64::consts::PI;

    let (r, a, d, tol) = (10.0_f64, 3.0_f64, 20.0_f64, 1e-6_f64);
    let can = make_cylinder([0.0; 3], r, 40.0).unwrap();
    let (_, order) = can.to_record().unwrap();
    let wall = face(&can, &order, None);

    // A single-span horizontal line crossing the seam at y = 0, z = 20.
    let chord = Curve::Line {
        start: [d, -a, 20.0],
        end: [d, a, 20.0],
    };
    let dropped_chord = drop_curves(&can, &order, wall, &[chord], tol).unwrap();
    assert_eq!(
        dropped_chord.len(),
        2,
        "line across seam drops as two pieces"
    );
    for (c, _) in &dropped_chord {
        let p = points(c);
        for q in [p[0], p[p.len() - 1]] {
            let rad = q[0].hypot(q[1]);
            assert!((rad - r).abs() < 1e-5, "endpoint on cylinder: {q:?}");
        }
        for q in &p {
            assert!((q[2] - 20.0).abs() < 1e-5, "at z = 20: {q:?}");
        }
    }
    let seam_hit = |c: &Curve, z: f64| {
        let p = points(c);
        [p[0], p[p.len() - 1]]
            .into_iter()
            .any(|q| (q[0] - r).abs() < 1e-5 && q[1].abs() < 1e-5 && (q[2] - z).abs() < 1e-5)
    };
    assert!(
        dropped_chord.iter().all(|(c, _)| seam_hit(c, 20.0)),
        "both pieces of the chord end on the seam"
    );
    // The two non-seam endpoints must match the radial projections of [d, -a, 20] and [d, a, 20]
    // (one each, order-independently), ruling out duplicated single-side pieces.
    let hyp = d.hypot(a);
    let want_neg = [r * d / hyp, -r * a / hyp, 20.0];
    let want_pos = [r * d / hyp, r * a / hyp, 20.0];
    let near = |p: [f64; 3], q: [f64; 3]| {
        (p[0] - q[0]).abs() < 1e-5 && (p[1] - q[1]).abs() < 1e-5 && (p[2] - q[2]).abs() < 1e-5
    };
    let mut non_seam_ends = Vec::with_capacity(2);
    for (c, _) in &dropped_chord {
        let p = points(c);
        let (a_end, b_end) = (p[0], p[p.len() - 1]);
        let other = if near(a_end, [r, 0.0, 20.0]) {
            b_end
        } else {
            a_end
        };
        non_seam_ends.push(other);
    }
    assert!(
        (near(non_seam_ends[0], want_neg) && near(non_seam_ends[1], want_pos))
            || (near(non_seam_ends[0], want_pos) && near(non_seam_ends[1], want_neg)),
        "chord non-seam endpoints must span both sides of seam: got {non_seam_ends:?}, want {want_neg:?} and {want_pos:?}"
    );

    // Slot facing +x across the seam (the repro of issue #20836): two straight sides at
    // y = +-3 and two semicircular ends about (20, 0, 12) and (20, 0, 28).
    let slot_facing = |n: [f64; 2]| {
        let at = |s: f64, z: f64| [n[0] * d + n[1] * s, n[1] * d - n[0] * s, z];
        let end = |z: f64, start: f64| Curve::Arc {
            center: at(0.0, z),
            axis: [-n[0], -n[1], 0.0],
            reference: [n[1], -n[0], 0.0],
            radius: a,
            start,
            end: start + PI,
        };
        [
            Curve::Line {
                start: at(a, 12.0),
                end: at(a, 28.0),
            },
            end(28.0, 0.0),
            Curve::Line {
                start: at(-a, 28.0),
                end: at(-a, 12.0),
            },
            end(12.0, PI),
        ]
    };

    let slot_x = slot_facing([1.0, 0.0]);
    // Each round end crosses the seam and drops as two pieces ending on the seam.
    for (idx, z_seam) in [(1, 28.0 + a), (3, 12.0 - a)] {
        let pieces = drop_curves(&can, &order, wall, &[slot_x[idx].clone()], tol).unwrap();
        assert_eq!(pieces.len(), 2, "round end {idx} drops as two pieces");
        assert!(
            pieces.iter().all(|(c, _)| seam_hit(c, z_seam)),
            "both pieces of round end {idx} end on the seam at z = {z_seam}"
        );
    }

    // Closed-form area of the slot dropped square onto the cylinder.
    let want_inside = 32.0 * r * (a / d).atan() + 2.0 * PI * r * ((a * a + d * d).sqrt() - d);
    let area_bound = tol * 2.0 * PI * a;
    for n in [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]] {
        let curves: Vec<Curve> = drop_curves(&can, &order, wall, &slot_facing(n), tol)
            .unwrap()
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        let (cut, regions) = split_face_by_curves(&can, &order, wall, &curves, tol).unwrap();
        let (_, cut_order) = cut.to_record().unwrap();
        let mut areas: Vec<f64> = regions
            .iter()
            .map(|reg| cut.face_area(&cut_order, reg.face, 1e-8).unwrap())
            .collect();
        areas.sort_by(f64::total_cmp);
        let inside_area = if n == [1.0, 0.0] {
            // Crossing the seam splits the slot along the cylinder's seam edge into two
            // symmetric halves plus the rest of the wall (3 faces total).
            assert_eq!(
                regions.len(),
                3,
                "facing +x across seam: two slot halves + wall"
            );
            assert!(
                (areas[0] - want_inside / 2.0).abs() < area_bound
                    && (areas[1] - want_inside / 2.0).abs() < area_bound,
                "facing +x: slot halves {:?} vs half closed form {}",
                &areas[..2],
                want_inside / 2.0
            );
            areas[0] + areas[1]
        } else {
            assert_eq!(regions.len(), 2, "facing {n:?}: two regions");
            areas[0]
        };
        assert!(
            (inside_area - want_inside).abs() < area_bound,
            "facing {n:?}: inside area {inside_area} vs closed form {want_inside}",
        );
    }
}
