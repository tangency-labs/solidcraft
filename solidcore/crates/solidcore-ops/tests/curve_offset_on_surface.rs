//! A curve on a sheet offset along it: a circle round a cylinder offset 30 along it stays on the
//! cylinder, 30 above or below; a line on a flat sheet gives the parallel line; an offset that
//! would leave the sheet and malformed asks are refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods // the test's own sampling and radii, not ops computing geometry
)]

use solidcore_ops::curve::{circle, offset_on_surface, OffsetSide};
use solidcore_ops::sweep::{loft, Curve as Sweep};
use solidcore_ops::Error;
use solidcore_record::brep::{Brep, Curve, Natural};

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

/// The ruled one-face sheet between two sweep curves, as a record.
fn sheet(a: Sweep, b: Sweep) -> Brep {
    let body = loft(&[vec![a], vec![b]], 1, false).unwrap();
    body.to_record().unwrap().0
}

/// A whole turn of radius 50 about z at height `z`, as a sweep curve.
fn turn(z: f64) -> Sweep {
    Sweep::Arc {
        center: [0.0, 0.0, z],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 50.0,
        start: 0.0,
        end: std::f64::consts::TAU,
    }
}

fn round(z: f64) -> Curve {
    circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 50.0).unwrap()
}

fn flat() -> Brep {
    sheet(
        Sweep::Line {
            start: [0.0, 0.0, 0.0],
            end: [20.0, 0.0, 0.0],
        },
        Sweep::Line {
            start: [0.0, 10.0, 0.0],
            end: [20.0, 10.0, 0.0],
        },
    )
}

/// A cylinder of radius 50 from z = 0 to 100, and its circle at z = 50 offset 30 each way along
/// it: every point of each offset lies on the cylinder, 30 above or below.
#[test]
fn a_circle_round_a_cylinder_offsets_along_it() {
    let wall = sheet(turn(0.0), turn(100.0));
    let mut zs = Vec::new();
    for side in [OffsetSide::Left, OffsetSide::Right] {
        let o = offset_on_surface(&round(50.0), &wall, 30.0, side, 1e-3).unwrap();
        assert!(o.deviation <= 1e-3, "{side:?}: {}", o.deviation);
        let pts = points_of(&o.curve, 80);
        for p in &pts {
            assert!((p[0].hypot(p[1]) - 50.0).abs() <= 1e-3, "{p:?}");
            assert!(((p[2] - 50.0).abs() - 30.0).abs() <= 1e-3, "{p:?}");
        }
        zs.push(pts[0][2]);
    }
    zs.sort_by(f64::total_cmp);
    assert!(
        (zs[0] - 20.0).abs() <= 1e-3 && (zs[1] - 80.0).abs() <= 1e-3,
        "{zs:?}"
    );
}

/// A line across a flat sheet offset 3: the parallel line, 3 from it in the sheet.
#[test]
fn a_line_on_a_flat_sheet_offsets_to_the_parallel_line() {
    let flat = flat();
    let line = Curve::Line {
        start: [2.0, 5.0, 0.0],
        end: [18.0, 5.0, 0.0],
    };
    for side in [OffsetSide::Left, OffsetSide::Right] {
        let o = offset_on_surface(&line, &flat, 3.0, side, 1e-6).unwrap();
        for p in points_of(&o.curve, 40) {
            assert!(
                ((p[1] - 5.0).abs() - 3.0).abs() <= 1e-6 && p[2].abs() <= 1e-9,
                "{p:?}"
            );
        }
    }
}

/// An offset that would run off the sheet, and malformed asks, are refused.
#[test]
fn malformed_and_off_sheet_offsets_are_refused() {
    let flat = flat();
    let near_edge = Curve::Line {
        start: [2.0, 9.0, 0.0],
        end: [18.0, 9.0, 0.0],
    };
    let left = offset_on_surface(&near_edge, &flat, 3.0, OffsetSide::Left, 1e-6);
    let right = offset_on_surface(&near_edge, &flat, 3.0, OffsetSide::Right, 1e-6);
    assert!(left.is_ok() != right.is_ok(), "{left:?} {right:?}");
    for (d, tol) in [(0.0, 1e-6), (-1.0, 1e-6), (f64::NAN, 1e-6), (1.0, 0.0)] {
        assert_eq!(
            offset_on_surface(&near_edge, &flat, d, OffsetSide::Left, tol).err(),
            Some(Error::Argument),
            "{d} {tol}"
        );
    }
}
