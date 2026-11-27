//! A sheet's edge matched to its own mirror through ops, on record sheets: half a deck whose
//! edge lies on the side plane `y = 0` but leaves it 20 degrees off square, made square so it
//! meets its mirror smoothly; an edge off the plane put on it; and the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::measure::point_distance;
use solidcore_ops::surface::ruled;
use solidcore_ops::surface_cvs::{
    control_net, match_plane, MatchOrder, MatchRows, MirrorPlane, SeamBounds,
};
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

/// The side plane of a car.
const SIDE: MirrorPlane = MirrorPlane {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 1.0, 0.0],
};

/// Half a deck from its edge on `y = 0` (at height `h`) out to `y = 10 cos 20`, rising 20
/// degrees as it goes (10 at 20 degrees: its cosine and sine as constants, ops computes no
/// trigonometry).
fn half_deck(h: f64) -> Brep {
    let (c, s) = (
        10.0 * 0.939_692_620_785_908_4,
        10.0 * 0.342_020_143_325_668_7,
    );
    ruled(
        &line([0.0, h, 0.0], [20.0, h, 0.0]),
        &line([0.0, c, s], [20.0, c, s]),
        false,
    )
    .unwrap()
}

/// The deck meets its mirror at a 40 degree crease; matched at G1 the seam with the mirror is
/// tangent and the edge stays on the plane, and the net's next row is square to the plane behind
/// the edge row.
#[test]
fn half_a_deck_is_made_square_to_its_mirror_plane() {
    let deck = half_deck(0.0);
    let edge = edge_through(&deck, [10.0, 0.0, 0.0]);
    let (matched, seam) =
        match_plane(&deck, edge, SIDE, MatchOrder::G1, rows(2, 0.5), held()).unwrap();
    assert!(seam.gap < 1e-9 && seam.angle < 1e-9, "{seam:?}");
    assert!(seam.curvature < 1e-9, "{seam:?}");
    edge_through(&matched, [10.0, 0.0, 0.0]);
    // the net: the edge's control points on the plane, and the one next to each across the edge
    // (the nearest off the plane) straight out from it
    let net = control_net(&matched).unwrap();
    let (on, off): (Vec<[f64; 3]>, Vec<[f64; 3]>) =
        net.points.iter().copied().partition(|p| p[1].abs() < 1e-9);
    assert!(on.len() >= 2, "{net:?}");
    for p in &on {
        let d = |q: &[f64; 3]| point_distance(*p, *q).unwrap().value;
        let q = off.iter().min_by(|a, b| d(a).total_cmp(&d(b))).unwrap();
        assert!(
            (q[0] - p[0]).abs() < 1e-9 && (q[2] - p[2]).abs() < 1e-9,
            "{p:?} {q:?}"
        );
        assert!(q[1] > 0.0, "{q:?}");
    }
}

/// The deck's edge 1 off the plane, matched at G2: the edge is put on the plane and the seam
/// with the mirror holds G2.
#[test]
fn an_edge_off_the_plane_is_put_on_it() {
    let deck = half_deck(1.0);
    let edge = edge_through(&deck, [10.0, 1.0, 0.0]);
    let (_, seam) = match_plane(&deck, edge, SIDE, MatchOrder::G2, rows(3, 0.5), held()).unwrap();
    assert!(seam.gap < 1e-9 && seam.angle < 1e-9, "{seam:?}");
    assert!(seam.curvature < 1e-9, "{seam:?}");
}

/// The refusals: a normal of no length or not finite, an edge that runs along the plane without
/// leaving it (the deck's own plane is no mirror for it), rows below what G1 fixes, and a floor
/// that is not finite.
#[test]
fn malformed_plane_matches_are_refused() {
    let deck = half_deck(0.0);
    let edge = edge_through(&deck, [10.0, 0.0, 0.0]);
    let with = |plane: MirrorPlane, order, r| match_plane(&deck, edge, plane, order, r, held());
    let flat_normal = |normal| MirrorPlane {
        origin: [0.0; 3],
        normal,
    };
    for normal in [[0.0; 3], [f64::NAN, 1.0, 0.0]] {
        assert_eq!(
            with(flat_normal(normal), MatchOrder::G1, rows(2, 0.5)).err(),
            Some(Error::Argument)
        );
    }
    // the plane through the edge square to z = 0 at the deck's 20 degrees: the deck lies in it
    let own = flat_normal([0.0, -0.342_020_143_325_668_7, 0.939_692_620_785_908_4]);
    assert_eq!(
        with(own, MatchOrder::G1, rows(2, 0.5)).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        with(SIDE, MatchOrder::G1, rows(1, 0.5)).err(),
        Some(Error::Argument)
    );
    let bad = SeamBounds {
        gap: f64::NAN,
        ..held()
    };
    assert_eq!(
        match_plane(&deck, edge, SIDE, MatchOrder::G1, rows(2, 0.5), bad).err(),
        Some(Error::Argument)
    );
}
