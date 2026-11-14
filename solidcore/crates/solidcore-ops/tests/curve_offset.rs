//! A planar curve offset in its plane: every point of the offset lies the distance asked from the
//! curve, measured; the two sides; a circle offset outward and inward; and the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods // the test's own sampling and radii, not ops computing geometry
)]

use solidcore_ops::curve::{closest_point, offset_planar, OffsetSide};

use solidcore_ops::Error;
use solidcore_record::brep::{Curve, Natural};

/// A planar cubic spline in z = 0, as a designer's few-point curve: a gentle S over x in 0..100.
fn s_curve() -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [0.0, 0.0, 0.0],
            [25.0, 20.0, 0.0],
            [50.0, 0.0, 0.0],
            [75.0, -20.0, 0.0],
            [100.0, 0.0, 0.0],
        ],
    }
}

/// `n + 1` points at even steps of `piece`'s own range, by the kernel's evaluation.
fn points_of(piece: &Curve, n: u32) -> Vec<[f64; 3]> {
    let k = solidcore::Kernel::default();
    let range = match (piece, piece.natural()) {
        (Curve::Line { .. }, _) => [0.0, 1.0],
        (_, Natural::Periodic) => [0.0, std::f64::consts::TAU],
        (_, Natural::Range(lo, hi)) => [lo, hi],
    };
    let record = solidcore_convert::curve(piece).unwrap();
    let made = solidcore::curve::from_parameters::from_parameters(&k, &record, range).unwrap();
    (0..=n)
        .map(|i| {
            let at = made
                .curve
                .evaluate_point(&k, made.interval.evaluate(f64::from(i) / f64::from(n)))
                .unwrap();
            [at.x, at.y, at.z]
        })
        .collect()
}

/// Each piece's points, sampled independently of the operation's own check, lie `distance`
/// from `source` within `tol`.
fn lies_at(pieces: &[Curve], source: &Curve, distance: f64, tol: f64) {
    assert!(!pieces.is_empty());
    for piece in pieces {
        for p in points_of(piece, 40) {
            let foot = closest_point(source, p, 1e-9).unwrap().unwrap();
            assert!(
                (foot.distance - distance).abs() <= tol,
                "at {p:?}: {} from the curve, {distance} asked",
                foot.distance
            );
        }
    }
}

/// A planar spline offset 5 to each side: every point of each result lies 5 from the curve,
/// within the tolerance asked, and the deviation the operation reports is within it too.
#[test]
fn a_planar_spline_offset_lies_the_distance_from_it() {
    let c = s_curve();
    for side in [OffsetSide::Left, OffsetSide::Right] {
        let o = offset_planar(&c, 5.0, side, 1e-3).unwrap();
        assert!(o.deviation <= 1e-3, "{side:?}: {}", o.deviation);
        lies_at(&o.curves, &c, 5.0, 1e-3);
    }
}

/// Left is to the left looking down the plane's normal the offset reports, right the other way:
/// a straight-ish run along +x has its left offset on the side `normal x (1, 0, 0)` points to.
#[test]
fn the_sides_are_left_and_right_of_the_curve_about_the_normal() {
    let c = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [50.0, 10.0, 0.0], [100.0, 0.0, 0.0]],
    };
    let left = offset_planar(&c, 2.0, OffsetSide::Left, 1e-3).unwrap();
    let right = offset_planar(&c, 2.0, OffsetSide::Right, 1e-3).unwrap();
    assert!(left.normal[2].abs() > 0.999, "{:?}", left.normal);
    assert_eq!(left.normal, right.normal);
    // normal x +x is +y for a normal along +z
    let y_of_left = left.normal[2].signum();
    let start = |o: &solidcore_ops::curve::PlanarOffset| points_of(&o.curves[0], 1)[0];
    // the offsets of the start (0, 0, 0), whose tangent leans up to (50, 10)
    assert!(start(&left)[1] * y_of_left > 1.0, "{:?}", start(&left));
    assert!(start(&right)[1] * y_of_left < -1.0, "{:?}", start(&right));
}

/// A circle of radius 10 offset 3 each way: the results lie 3 from it, at radius 13 and 7.
#[test]
fn a_circle_offsets_outward_and_inward() {
    let c = solidcore_ops::curve::circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 10.0).unwrap();
    let mut radii = Vec::new();
    for side in [OffsetSide::Left, OffsetSide::Right] {
        let o = offset_planar(&c, 3.0, side, 1e-4).unwrap();
        lies_at(&o.curves, &c, 3.0, 1e-4);
        let p = points_of(&o.curves[0], 1)[0];
        radii.push(p[0].hypot(p[1]));
    }
    radii.sort_by(f64::total_cmp);
    assert!(
        (radii[0] - 7.0).abs() <= 1e-3 && (radii[1] - 13.0).abs() <= 1e-3,
        "{radii:?}"
    );
}

/// The refusals: a distance or tolerance not finite and positive, a curve in no one plane (a
/// twisted spline, a straight line), and a curve the record refuses.
#[test]
fn malformed_offsets_are_refused() {
    let c = s_curve();
    for (d, tol) in [
        (0.0, 1e-3),
        (-1.0, 1e-3),
        (f64::NAN, 1e-3),
        (1.0, 0.0),
        (1.0, f64::NAN),
    ] {
        assert_eq!(
            offset_planar(&c, d, OffsetSide::Left, tol).err(),
            Some(Error::Argument),
            "{d} {tol}"
        );
    }
    let twisted = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [20.0, 0.0, 10.0],
            [30.0, 10.0, 25.0],
        ],
    };
    let line = Curve::Line {
        start: [0.0; 3],
        end: [10.0, 0.0, 0.0],
    };
    for c in [twisted, line] {
        assert!(
            matches!(
                offset_planar(&c, 1.0, OffsetSide::Left, 1e-3),
                Err(Error::Unsupported(_))
            ),
            "{c:?}"
        );
    }
}

/// Offset past the tightest bend (the S bends tighter than 30 on its inner sides): the loops
/// the offset would make are trimmed off, and what is left lies 30 from the curve.
#[test]
fn an_offset_past_a_bend_is_trimmed_where_it_would_cross_itself() {
    let c = s_curve();
    // the S's tightest bend: a radius below 30
    let comb = solidcore_ops::curve::curve_comb(&c, [0.0, 1.0], 401).unwrap();
    let most = comb
        .iter()
        .map(|t| t.curvature.iter().map(|x| x * x).sum::<f64>().sqrt())
        .fold(0.0, f64::max);
    assert!(1.0 / most < 30.0, "tightest radius {}", 1.0 / most);
    for side in [OffsetSide::Left, OffsetSide::Right] {
        let o = offset_planar(&c, 30.0, side, 1e-3).unwrap();
        lies_at(&o.curves, &c, 30.0, 1e-3);
    }
}
