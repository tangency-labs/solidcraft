//! A sheet's edge matched to another sheet's edge through ops, on record sheets: a 2 degree kink
//! taken out at G1, a curvature jump taken out at G2 against a curved sheet, how far one row and
//! three rows carry a G0 move, and the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::surface::ruled;
use solidcore_ops::surface_cvs::{control_net, match_edge, MatchOrder, MatchRows, SeamBounds};
use solidcore_ops::{Body, Error};
use solidcore_record::brep::{Brep, Curve};

/// A straight line as a record spline.
fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
    }
}

/// The record edge of `sheet` through `p`.
fn edge_through(sheet: &Brep, p: [f64; 3]) -> usize {
    let (body, order) = Body::from_record(sheet, true).unwrap();
    (0..order.edges.len())
        .find(|&i| {
            body.edge_closest_point(&order, i, p)
                .unwrap()
                .is_some_and(|at| at.distance < 1e-9)
        })
        .unwrap_or_else(|| panic!("no edge through {p:?}"))
}

/// A flat 10 by 10 sheet in `z = h` from `x = 0` to `x = 10`.
fn flat(h: f64) -> Brep {
    ruled(
        &line([10.0, 0.0, h], [10.0, 10.0, h]),
        &line([0.0, 0.0, h], [0.0, 10.0, h]),
        false,
    )
    .unwrap()
}

/// What counts as held: a nanometre, a nanoradian, a nano per millimetre.
fn held() -> SeamBounds {
    SeamBounds {
        gap: 1e-9,
        angle: 1e-9,
        curvature: 1e-9,
    }
}

fn rows(n: usize, blend: f64) -> MatchRows {
    MatchRows { rows: n, blend }
}

/// The flat sheet's edge at `x = 0` matched at G1 to a sheet coming up to it 2 degrees off its
/// plane: the seam after is tangent.
#[test]
fn a_two_degree_kink_is_taken_out_at_g1() {
    // 10 mm at 2 degrees: its cosine and sine as constants (ops computes no trigonometry)
    let (c, s) = (
        10.0 * 0.999_390_827_019_095_8,
        10.0 * 0.034_899_496_702_500_97,
    );
    let sheet = flat(0.0);
    let neighbour = ruled(
        &line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        &line([-c, 0.0, s], [-c, 10.0, s]),
        false,
    )
    .unwrap();
    let edge = edge_through(&sheet, [0.0, 5.0, 0.0]);
    let target = edge_through(&neighbour, [0.0, 5.0, 0.0]);
    let (matched, seam) = match_edge(
        &sheet,
        edge,
        &neighbour,
        target,
        MatchOrder::G1,
        rows(2, 0.5),
        held(),
    )
    .unwrap();
    assert!(seam.angle < 1e-9, "{seam:?}");
    assert!(seam.gap < 1e-9, "{seam:?}");
    // the result is a one-face sheet on a refined net
    let net = control_net(&matched).unwrap();
    assert!(net.rows * net.cols > 4, "{net:?}");
}

/// Against a curved sheet (parabolic arcs, end tangent level, end curvature 1/25) the flat sheet
/// is G1 already; matched at G2, its curvature across the seam is the arc's.
#[test]
fn a_curvature_jump_is_taken_out_at_g2() {
    let arc = |y: f64| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: vec![[-10.0, y, -2.0], [-5.0, y, 0.0], [0.0, y, 0.0]],
    };
    let arch = ruled(&arc(0.0), &arc(10.0), false).unwrap();
    let sheet = flat(0.0);
    let edge = edge_through(&sheet, [0.0, 5.0, 0.0]);
    let target = edge_through(&arch, [0.0, 5.0, 0.0]);
    let (_, seam) = match_edge(
        &sheet,
        edge,
        &arch,
        target,
        MatchOrder::G2,
        rows(3, 0.5),
        held(),
    )
    .unwrap();
    assert!(seam.curvature < 1e-9, "{seam:?}");
    assert!(seam.angle < 1e-9, "{seam:?}");
    assert!(seam.gap < 1e-9, "{seam:?}");
}

/// The flat sheet 1 above its neighbour's edge, matched at G0: with one row the sheet bends at
/// its edge alone; with three rows the move reaches further in. Seen at a point a fifth of the
/// way across: unmoved with one row, moved with three.
#[test]
fn three_rows_carry_a_move_further_than_one() {
    let below = ruled(
        &line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        &line([-10.0, 0.0, 0.0], [-10.0, 10.0, 0.0]),
        false,
    )
    .unwrap();
    let sheet = flat(1.0);
    let edge = edge_through(&sheet, [0.0, 5.0, 1.0]);
    let target = edge_through(&below, [0.0, 5.0, 0.0]);
    let lowest = |m: &Brep| {
        control_net(m)
            .unwrap()
            .points
            .iter()
            .filter(|p| p[2] < 1.0 - 1e-12)
            .count()
    };
    let (one, seam) = match_edge(
        &sheet,
        edge,
        &below,
        target,
        MatchOrder::G0,
        rows(1, 0.5),
        held(),
    )
    .unwrap();
    assert!(seam.gap < 1e-9, "{seam:?}");
    let (three, _) = match_edge(
        &sheet,
        edge,
        &below,
        target,
        MatchOrder::G0,
        rows(3, 0.5),
        held(),
    )
    .unwrap();
    // one row moves the edge's points only; three move three rows' worth
    let (n1, n3) = (lowest(&one), lowest(&three));
    assert!(n3 >= 3 * n1, "{n1} against {n3}");
}

/// The refusals: an edge past the record, rows below the order, a negative floor.
#[test]
fn malformed_matches_are_refused() {
    let sheet = flat(0.0);
    let other = flat(0.0);
    let edge = edge_through(&sheet, [0.0, 5.0, 0.0]);
    let far = edge_through(&other, [10.0, 5.0, 0.0]);
    assert_eq!(
        match_edge(
            &sheet,
            99,
            &other,
            far,
            MatchOrder::G1,
            rows(2, 0.5),
            held()
        )
        .err(),
        Some(Error::Argument)
    );
    assert_eq!(
        match_edge(
            &sheet,
            edge,
            &other,
            far,
            MatchOrder::G2,
            rows(2, 0.5),
            held()
        )
        .err(),
        Some(Error::Argument)
    );
    let negative = SeamBounds {
        gap: -1.0,
        ..held()
    };
    assert_eq!(
        match_edge(
            &sheet,
            edge,
            &other,
            far,
            MatchOrder::G0,
            rows(1, 0.5),
            negative
        )
        .err(),
        Some(Error::Argument)
    );
}
