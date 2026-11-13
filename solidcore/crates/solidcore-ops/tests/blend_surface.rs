//! A blend sheet between an edge of one sheet and an edge of another, each side at its own
//! continuity, with the seams it makes measured: between two flat sheets at an angle, held G0,
//! G1 and G2; between two curved sheets, held G2 at both; and the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::blend_surface::{blend_edges, BlendEdges, BlendSide};
use solidcore_ops::continuity::{ContinuityTolerances, SeamReport};
use solidcore_ops::sweep::railed::Continuity;
use solidcore_ops::sweep::{loft, Curve};
use solidcore_ops::{Body, Error, Order};

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// The ruled sheet between two curves.
fn sheet(a: Curve, b: Curve) -> (Body, Order) {
    let body = loft(&[vec![a], vec![b]], 1, false).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// The record edge of `body` through `p`.
fn edge_through(body: &Body, order: &Order, p: [f64; 3]) -> usize {
    (0..order.edges.len())
        .find(|&i| {
            body.edge_closest_point(order, i, p)
                .unwrap()
                .is_some_and(|at| at.distance < 1e-9)
        })
        .unwrap_or_else(|| panic!("no edge through {p:?}"))
}

/// A micron, a hundredth of a degree, and a thousandth per millimetre for the curvature jumps.
fn tight() -> ContinuityTolerances {
    ContinuityTolerances {
        gap_mm: 1e-3,
        angle_rad: std::f64::consts::PI / 18_000.0,
        curvature_per_mm: 1e-3,
        curvature_change_per_mm2: 1e-3,
        curvature_change_rate_per_mm3: 1e-3,
    }
}

fn how() -> BlendEdges {
    BlendEdges {
        stations: 9,
        min_in_plane: 0.2,
        crown: [0.0, 0.0],
        stitch_tolerance: 1e-6,
        seam: tight(),
    }
}

/// A flat 10 by 10 sheet in z = 0 for x in 0..10, and another 10 wide, rising at 45 degrees
/// from the line x = 15, z = 0: their facing edges 5 apart, parallel along y.
fn flat_pair() -> ((Body, Order, usize), (Body, Order, usize)) {
    let (a, ao) = sheet(
        line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
    );
    let ea = edge_through(&a, &ao, [10.0, 5.0, 0.0]);
    let r = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let (b, bo) = sheet(
        line([15.0, 0.0, 0.0], [15.0, 10.0, 0.0]),
        line([15.0 + r, 0.0, r], [15.0 + r, 10.0, r]),
    );
    let eb = edge_through(&b, &bo, [15.0, 5.0, 0.0]);
    ((a, ao, ea), (b, bo, eb))
}

fn side(s: &(Body, Order, usize), continuity: Continuity) -> BlendSide<'_> {
    BlendSide {
        body: &s.0,
        order: &s.1,
        edge: s.2,
        continuity,
    }
}

/// Every station of a seam within `tight()`'s gap, and the seam holding `order` at its stations.
fn holds(seam: &SeamReport, order: usize) {
    assert!(!seam.stations.is_empty(), "{seam:?}");
    for st in &seam.stations {
        assert!(st.gap_mm <= 1e-6, "{st:?}");
    }
    assert!(
        seam.order_at_stations.is_some_and(|o| o >= order),
        "G{order} asked: {:?}, worst {:?}",
        seam.order_at_stations,
        seam.stations[seam.worst[order.min(4)]]
    );
}

/// Held G0 at both, the blend between the two straight edges is the flat strip between them:
/// 5 by 10, in z = 0, meeting the first sheet flat and the second at 45 degrees.
#[test]
fn a_g0_blend_of_two_flat_sheets_is_the_strip_between_their_edges() {
    let (a, b) = flat_pair();
    let (blend, [sa, sb]) =
        blend_edges(side(&a, Continuity::G0), side(&b, Continuity::G0), &how()).unwrap();
    assert_eq!(blend.counts().unwrap()[0], 1);
    let area = blend.area(1e-10).unwrap();
    assert!((area - 50.0).abs() <= 1e-6, "{area}");
    holds(&sa, 0);
    holds(&sb, 0);
    for st in &sb.stations {
        assert!((st.normal_angle_deg - 45.0).abs() <= 1e-6, "{st:?}");
    }
}

/// Held G1 at both, the blend leaves each sheet along it: both seams tangent at every station,
/// and the kernel proves the seams tangent all along at the caller's angle.
#[test]
fn a_g1_blend_of_two_flat_sheets_is_tangent_to_both() {
    let (a, b) = flat_pair();
    let (_, [sa, sb]) =
        blend_edges(side(&a, Continuity::G1), side(&b, Continuity::G1), &how()).unwrap();
    holds(&sa, 1);
    holds(&sb, 1);
}

/// Held G2 at both, the blend leaves each flat sheet with no curvature across the seam.
#[test]
fn a_g2_blend_of_two_flat_sheets_is_curvature_continuous_with_both() {
    let (a, b) = flat_pair();
    let (_, [sa, sb]) =
        blend_edges(side(&a, Continuity::G2), side(&b, Continuity::G2), &how()).unwrap();
    holds(&sa, 2);
    holds(&sb, 2);
}

/// Each side at its own order: G2 to the first sheet, G0 to the second (a crease of 45
/// degrees there at most).
#[test]
fn each_side_holds_its_own_order() {
    let (a, b) = flat_pair();
    let (_, [sa, sb]) =
        blend_edges(side(&a, Continuity::G2), side(&b, Continuity::G0), &how()).unwrap();
    holds(&sa, 2);
    holds(&sb, 0);
}

/// The second sheet's edge running the other way: the blend still pairs end with nearer end,
/// so it is the same flat strip.
#[test]
fn an_edge_running_the_other_way_is_paired_end_to_nearer_end() {
    let (a, ao) = sheet(
        line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
    );
    let ea = edge_through(&a, &ao, [10.0, 5.0, 0.0]);
    let (b, bo) = sheet(
        line([15.0, 10.0, 0.0], [15.0, 0.0, 0.0]),
        line([25.0, 10.0, 0.0], [25.0, 0.0, 0.0]),
    );
    let eb = edge_through(&b, &bo, [15.0, 5.0, 0.0]);
    let (a, b) = ((a, ao, ea), (b, bo, eb));
    let (blend, _) =
        blend_edges(side(&a, Continuity::G0), side(&b, Continuity::G0), &how()).unwrap();
    let area = blend.area(1e-10).unwrap();
    assert!((area - 50.0).abs() <= 1e-6, "{area}");
}

/// A curved sheet: the ruled sheet between two parabolic arcs, the same arc at y = 0 and
/// y = 10, crowning 2 above the line from `x0` to `x0 + 10`.
fn arch(x0: f64) -> (Body, Order) {
    let arc = |y: f64| Curve::Nurbs {
        degree: 2,
        points: vec![
            [x0, y, 0.0, 1.0],
            [x0 + 5.0, y, 4.0, 1.0],
            [x0 + 10.0, y, 0.0, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    };
    sheet(arc(0.0), arc(10.0))
}

/// Two arches side by side, 10 apart, blended G2 from the first's right edge to the second's
/// left: the arches have the same section all along y, so the blend holds G2 all along each
/// seam, between the stations too.
#[test]
fn a_g2_blend_of_two_curved_sheets_is_curvature_continuous_with_both() {
    let (a, ao) = arch(0.0);
    let ea = edge_through(&a, &ao, [10.0, 5.0, 0.0]);
    let (b, bo) = arch(20.0);
    let eb = edge_through(&b, &bo, [20.0, 5.0, 0.0]);
    let (a, b) = ((a, ao, ea), (b, bo, eb));
    let (blend, [sa, sb]) =
        blend_edges(side(&a, Continuity::G2), side(&b, Continuity::G2), &how()).unwrap();
    assert_eq!(blend.counts().unwrap()[0], 1);
    holds(&sa, 2);
    holds(&sb, 2);
}

/// The refusals: an edge past the record, stations and tolerances out of range, and the same
/// edge on both sides (no direction to leave in).
#[test]
fn malformed_blends_are_refused() {
    let (a, b) = flat_pair();
    let bad_edge = BlendSide {
        edge: 999,
        ..side(&a, Continuity::G1)
    };
    assert_eq!(
        blend_edges(bad_edge, side(&b, Continuity::G1), &how()).err(),
        Some(Error::Argument)
    );
    for bad in [
        BlendEdges {
            stations: 2,
            ..how()
        },
        BlendEdges {
            stations: 258,
            ..how()
        },
        BlendEdges {
            min_in_plane: 1.5,
            ..how()
        },
        BlendEdges {
            crown: [4.0, 0.0],
            ..how()
        },
        BlendEdges {
            stitch_tolerance: 0.0,
            ..how()
        },
        BlendEdges {
            seam: ContinuityTolerances {
                gap_mm: f64::NAN,
                ..tight()
            },
            ..how()
        },
    ] {
        assert_eq!(
            blend_edges(side(&a, Continuity::G1), side(&b, Continuity::G1), &bad).err(),
            Some(Error::Argument),
            "{bad:?}"
        );
    }
    assert!(blend_edges(side(&a, Continuity::G1), side(&a, Continuity::G1), &how()).is_err());
}
