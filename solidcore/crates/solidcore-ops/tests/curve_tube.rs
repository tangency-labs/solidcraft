//! The tube round a curve on a sheet meets the sheet in the chamfer's two boundaries: a line
//! across a flat sheet gives the two lines `radius` either side of it; a circle round a cylinder
//! gives the circles `radius` above and below it; a tube that misses the sheet and malformed
//! asks are refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods // the test's own sampling and radii, not ops computing geometry
)]

use solidcore_ops::curve::{circle, tube_on_surface};
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

/// The flat sheet x, y in 0..100 at z = 0.
fn flat() -> Brep {
    sheet(
        Sweep::Line {
            start: [0.0, 0.0, 0.0],
            end: [100.0, 0.0, 0.0],
        },
        Sweep::Line {
            start: [0.0, 100.0, 0.0],
            end: [100.0, 100.0, 0.0],
        },
    )
}

/// A straight run from `a` to `b` as a degree-1 spline.
fn run(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
    }
}

/// A run across the flat sheet at y = 50 and a tube of radius 5: the two lines at y = 45 and
/// y = 55, in the sheet, along the run.
#[test]
fn a_tube_round_a_run_on_a_flat_sheet_meets_it_either_side() {
    let c = run([10.0, 50.0, 0.0], [90.0, 50.0, 0.0]);
    let found = tube_on_surface(&c, &flat(), 5.0, 1e-4).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    let mut sides = Vec::new();
    for f in &found {
        let pts = points_of(f, 40);
        for p in &pts {
            assert!(p[2].abs() <= 1e-4, "{p:?}");
            assert!(((p[1] - 50.0).abs() - 5.0).abs() <= 1e-4, "{p:?}");
            assert!((10.0 - 1e-4..=90.0 + 1e-4).contains(&p[0]), "{p:?}");
        }
        sides.push(pts[0][1]);
    }
    sides.sort_by(f64::total_cmp);
    assert!(
        (sides[0] - 45.0).abs() <= 1e-4 && (sides[1] - 55.0).abs() <= 1e-4,
        "{sides:?}"
    );
}

/// A cylinder of radius 50 from z = 0 to 100 and its circle at z = 50: the tube of radius 5
/// round the circle meets the cylinder in the circles at z = 45 and 55.
#[test]
fn a_tube_round_a_circle_on_a_cylinder_meets_it_above_and_below() {
    let turn = |z: f64| Sweep::Arc {
        center: [0.0, 0.0, z],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 50.0,
        start: 0.0,
        end: std::f64::consts::TAU,
    };
    let wall = sheet(turn(0.0), turn(100.0));
    let round = circle([0.0, 0.0, 50.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 50.0).unwrap();
    let found = tube_on_surface(&round, &wall, 5.0, 1e-3).unwrap();
    let mut heights = Vec::new();
    for f in &found {
        let pts = points_of(f, 60);
        for p in &pts {
            assert!((p[0].hypot(p[1]) - 50.0).abs() <= 1e-3, "{p:?}");
            assert!(((p[2] - 50.0).abs() - 5.0).abs() <= 1e-3, "{p:?}");
        }
        heights.push(pts[0][2]);
    }
    heights.sort_by(f64::total_cmp);
    assert!(
        heights.iter().any(|z| (z - 45.0).abs() <= 1e-3),
        "{heights:?}"
    );
    assert!(
        heights.iter().any(|z| (z - 55.0).abs() <= 1e-3),
        "{heights:?}"
    );
}

/// A tube that misses the sheet, and malformed asks, are refused.
#[test]
fn a_missing_tube_and_malformed_asks_are_refused() {
    let above = run([10.0, 50.0, 20.0], [90.0, 50.0, 20.0]);
    assert_eq!(
        tube_on_surface(&above, &flat(), 5.0, 1e-4).err(),
        Some(Error::Empty)
    );
    let c = run([10.0, 50.0, 0.0], [90.0, 50.0, 0.0]);
    for (r, tol) in [(0.0, 1e-4), (-1.0, 1e-4), (f64::NAN, 1e-4), (1.0, 0.0)] {
        assert_eq!(
            tube_on_surface(&c, &flat(), r, tol).err(),
            Some(Error::Argument),
            "{r} {tol}"
        );
    }
}
