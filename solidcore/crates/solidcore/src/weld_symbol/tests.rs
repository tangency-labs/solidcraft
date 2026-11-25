//! Tests of the weld symbol constructions: the contour mark's ends, radius and bulge, the V
//! groove's height, and refusals.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_PI_3, FRAC_PI_4};

use crate::math::{Point2, Vec2};
use crate::prism::profile::Seg2;
use crate::weld_symbol::*;

const G: f64 = 4.0;

fn flush(face: Point2, tangent: Vec2, outward: Vec2) -> (Point2, Point2) {
    match weld_contour_mark(face, tangent, outward, G, WeldContour::Flush).unwrap() {
        Seg2::Line { a, b } => (a, b),
        other => panic!("expected a line, got {other:?}"),
    }
}

fn arc(
    face: Point2,
    tangent: Vec2,
    outward: Vec2,
    contour: WeldContour,
) -> (Point2, f64, f64, f64) {
    match weld_contour_mark(face, tangent, outward, G, contour).unwrap() {
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => (center, radius, start, sweep),
        other => panic!("expected an arc, got {other:?}"),
    }
}

fn point_at(center: Point2, radius: f64, angle: f64) -> Point2 {
    center + Vec2::new(angle.cos(), angle.sin()) * radius
}

/// A butt face on the x axis facing up, and a fillet face tilted a quarter of a right angle.
fn faces() -> Vec<(Point2, Vec2, Vec2)> {
    let d = 1.0 / 2.0_f64.sqrt();
    vec![
        (
            Point2::new(1.0, 2.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
        ),
        (
            Point2::new(1.0, -2.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, -1.0),
        ),
        (Point2::new(0.5, 0.5), Vec2::new(d, -d), Vec2::new(d, d)),
        (Point2::new(0.5, -0.5), Vec2::new(d, d), Vec2::new(d, -d)),
    ]
}

#[test]
fn flush_mark_is_the_chord_centred_off_the_face() {
    for (face, tangent, outward) in faces() {
        let (a, b) = flush(face, tangent, outward);
        assert!(((b - a).length() - 1.5 * G).abs() < 1e-12);
        let middle = (a + b) * 0.5;
        let expected = face + outward * (0.5 * G + 1.5 * G - 1.5 * G * 3.0_f64.sqrt() / 2.0);
        assert!((middle - expected).length() < 1e-12);
        assert!(((b - a) / (b - a).length() - tangent).length() < 1e-12);
    }
}

#[test]
fn arcs_end_where_the_flush_line_ends() {
    for (face, tangent, outward) in faces() {
        let (a, b) = flush(face, tangent, outward);
        for contour in [WeldContour::Convex, WeldContour::Concave] {
            let (center, radius, start, sweep) = arc(face, tangent, outward, contour);
            let first = point_at(center, radius, start);
            let last = point_at(center, radius, start + sweep);
            let (near, far) = if (first - a).length() < (first - b).length() {
                (a, b)
            } else {
                (b, a)
            };
            assert!((first - near).length() < 1e-9, "{contour:?}");
            assert!((last - far).length() < 1e-9, "{contour:?}");
        }
    }
}

#[test]
fn arc_radius_is_one_and_a_half_times_the_size_and_sweep_sixty_degrees() {
    for (face, tangent, outward) in faces() {
        for contour in [WeldContour::Convex, WeldContour::Concave] {
            let (center, radius, start, sweep) = arc(face, tangent, outward, contour);
            assert_eq!(radius, 1.5 * G);
            assert!((sweep.abs() - FRAC_PI_3).abs() < 1e-15);
            let chord = (point_at(center, radius, start + sweep) - point_at(center, radius, start))
                .length();
            assert!(
                (chord - 1.5 * G).abs() < 1e-9,
                "sixty degrees: the chord equals the radius"
            );
            assert!((chord - radius).abs() < 1e-9);
        }
    }
}

#[test]
fn convex_bulges_outward_and_concave_inward() {
    for (face, tangent, outward) in faces() {
        let (a, b) = flush(face, tangent, outward);
        let chord_middle = (a + b) * 0.5;
        for (contour, side) in [(WeldContour::Convex, 1.0), (WeldContour::Concave, -1.0)] {
            let (center, radius, start, sweep) = arc(face, tangent, outward, contour);
            let crown = point_at(center, radius, start + sweep / 2.0);
            let bulge = (crown - chord_middle).dot(outward);
            assert!(side * bulge > 0.0, "{contour:?} bulge {bulge}");
            // The sagitta of a sixty-degree arc is r (1 - cos 30 degrees).
            assert!((bulge.abs() - radius * (1.0 - (FRAC_PI_3 / 2.0).cos())).abs() < 1e-9);
        }
    }
}

#[test]
fn tilt_comes_from_the_tangent() {
    let d = 1.0 / 2.0_f64.sqrt();
    // The exact quarter-right-angle tilt a fillet face has, not a nearby angle.
    let (_, _, start, _) = arc(
        Point2::ZERO,
        Vec2::new(d, -d),
        Vec2::new(d, d),
        WeldContour::Convex,
    );
    assert_eq!(start, 2.0 * FRAC_PI_3 - FRAC_PI_4);
    let (_, _, start, _) = arc(
        Point2::ZERO,
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
        WeldContour::Convex,
    );
    assert_eq!(start, 2.0 * FRAC_PI_3);
}

#[test]
fn a_direction_of_any_length_is_its_own_direction() {
    let d = 1.0 / 2.0_f64.sqrt();
    for contour in [
        WeldContour::Flush,
        WeldContour::Convex,
        WeldContour::Concave,
    ] {
        // The fillet face's diagonals, as unit vectors and as (1, -1) and (1, 1): the same mark,
        // to the bit, since 1 / sqrt(2) is each component over the length.
        let unit = weld_contour_mark(Point2::ZERO, Vec2::new(d, -d), Vec2::new(d, d), G, contour);
        let long = weld_contour_mark(
            Point2::ZERO,
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            G,
            contour,
        );
        assert_eq!(unit.unwrap(), long.unwrap(), "{contour:?}");
        let scaled = weld_contour_mark(
            Point2::ZERO,
            Vec2::new(2.0, 0.0),
            Vec2::new(0.0, 3.0),
            G,
            contour,
        );
        let plain = weld_contour_mark(
            Point2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            G,
            contour,
        );
        assert_eq!(scaled.unwrap(), plain.unwrap(), "{contour:?}");
    }
}

#[test]
fn v_groove_height_of_an_equilateral_groove() {
    for width in [1e-6, 1.0, 5.0, 123.4] {
        let height = v_groove_height(width).unwrap();
        // Two sides of length `w` and an opening `w` make an equilateral triangle.
        let side = (height * height + (width / 2.0).powi(2)).sqrt();
        assert!((side - width).abs() < 1e-9 * width.max(1.0));
    }
}

#[test]
fn hostile_input_is_refused() {
    let face = Point2::ZERO;
    let (t, n) = (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
    let bad_sizes = [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for size in bad_sizes {
        assert!(weld_contour_mark(face, t, n, size, WeldContour::Flush).is_err());
        assert!(v_groove_height(size).is_err());
    }
    let bad = [
        (Point2::new(f64::NAN, 0.0), t, n),
        (face, Vec2::new(f64::NAN, 0.0), n),
        (face, t, Vec2::new(0.0, f64::INFINITY)),
        (face, Vec2::ZERO, n),
        (face, t, Vec2::ZERO),
        (face, t, t),
        (face, t, Vec2::new(0.6, 0.8)),
    ];
    for (face, tangent, outward) in bad {
        for contour in [
            WeldContour::Flush,
            WeldContour::Convex,
            WeldContour::Concave,
        ] {
            assert!(
                weld_contour_mark(face, tangent, outward, G, contour).is_err(),
                "{face:?} {tangent:?} {outward:?}"
            );
        }
    }
}
