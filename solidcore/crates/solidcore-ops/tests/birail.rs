//! One profile swept along two rails through ops (`sweep::birail::sweep_two_rails`): a line
//! between parallel line rails is a planar strip, exact; diverging line rails scale a bump in
//! closed form (Scale: wider and higher; Stretch: wider only); a curved pair is certified within
//! the tolerance; a closed profile is refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::sweep::birail::sweep_two_rails;
use solidcore_ops::sweep::railed::RailFit;
use solidcore_ops::sweep::Curve;
use solidcore_ops::{Body, Error, Order};

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// The cubic Bezier on `points`.
fn cubic(points: [[f64; 3]; 4]) -> Curve {
    Curve::Nurbs {
        degree: 3,
        points: points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}

/// A bump across x from 0 to 10 in the plane y = 0.
const BUMP: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [3.0, 0.0, 2.0],
    [7.0, 0.0, 2.0],
    [10.0, 0.0, 0.0],
];

/// The bump at its parameter `u`, its x scaled by `kx`, its z by `kz`, at y = `y`.
fn bump_at(u: f64, kx: f64, kz: f64, y: f64) -> [f64; 3] {
    let v = 1.0 - u;
    let b = [v * v * v, 3.0 * u * v * v, 3.0 * u * u * v, u * u * u];
    let mut q = [0.0, y, 0.0];
    for (w, c) in b.iter().zip(BUMP) {
        q[0] += w * c[0] * kx;
        q[2] += w * c[2] * kz;
    }
    q
}

/// Whether some edge of `body` passes within `tol` of `p`.
fn on_an_edge(body: &Body, order: &Order, p: [f64; 3], tol: f64) -> bool {
    (0..order.edges.len()).any(|i| {
        body.edge_closest_point(order, i, p)
            .unwrap()
            .is_some_and(|at| at.distance <= tol)
    })
}

#[test]
fn a_line_between_parallel_rails_is_a_planar_strip() {
    let profile = [line([0.0; 3], [10.0, 0.0, 0.0])];
    let (r1, r2) = (
        line([0.0; 3], [0.0, 20.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 20.0, 0.0]),
    );
    let (body, bound) = sweep_two_rails(&profile, [&r1, &r2], RailFit::Scale, 1e-6).unwrap();
    assert!(bound <= 1e-9, "{bound}");
    let (record, order) = body.to_record().unwrap();
    assert!(record.vertices.iter().all(|v| v.point[2].abs() <= 1e-12));
    assert!(on_an_edge(&body, &order, [5.0, 20.0, 0.0], 1e-9));
}

#[test]
fn diverging_rails_scale_the_profile_in_closed_form() {
    let profile = [cubic(BUMP)];
    let (r1, r2) = (
        line([0.0; 3], [0.0, 20.0, 0.0]),
        line([10.0, 0.0, 0.0], [20.0, 20.0, 0.0]),
    );
    for (fit, kz) in [(RailFit::Scale, 2.0), (RailFit::Stretch, 1.0)] {
        let (body, bound) = sweep_two_rails(&profile, [&r1, &r2], fit, 1e-6).unwrap();
        assert!(bound <= 1e-6, "{fit:?}: {bound}");
        let (_, order) = body.to_record().unwrap();
        for i in 0..=10 {
            let q = bump_at(f64::from(i) / 10.0, 2.0, kz, 20.0);
            assert!(on_an_edge(&body, &order, q, 1e-9), "{fit:?}: {q:?}");
        }
    }
}

#[test]
fn a_curved_pair_is_certified_within_the_tolerance() {
    let profile = [cubic(BUMP)];
    let r1 = cubic([
        [0.0, 0.0, 0.0],
        [0.0, 7.0, 1.0],
        [-1.0, 14.0, -1.0],
        [0.0, 20.0, 0.0],
    ]);
    let r2 = cubic([
        [10.0, 0.0, 0.0],
        [12.0, 7.0, 0.0],
        [13.0, 14.0, 1.0],
        [12.0, 20.0, 0.0],
    ]);
    let tol = 1e-3;
    let (body, bound) = sweep_two_rails(&profile, [&r1, &r2], RailFit::Scale, tol).unwrap();
    assert!(bound <= tol, "{bound}");
    let (_, order) = body.to_record().unwrap();
    // the rails' far ends are the sheet's corners
    assert!(on_an_edge(&body, &order, [0.0, 20.0, 0.0], 1e-9));
    assert!(on_an_edge(&body, &order, [12.0, 20.0, 0.0], 1e-9));
}

#[test]
fn a_closed_profile_is_refused() {
    let square = [
        line([0.0; 3], [10.0, 0.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 0.0, 10.0]),
        line([10.0, 0.0, 10.0], [0.0, 0.0, 10.0]),
        line([0.0, 0.0, 10.0], [0.0; 3]),
    ];
    let (r1, r2) = (
        line([0.0; 3], [0.0, 20.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 20.0, 0.0]),
    );
    assert!(matches!(
        sweep_two_rails(&square, [&r1, &r2], RailFit::Scale, 1e-6),
        Err(Error::Unsupported(_))
    ));
}
