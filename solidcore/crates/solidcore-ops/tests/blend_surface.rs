//! A blend sheet between an edge of one sheet and an edge of another, each side at its own
//! continuity, with the seams it makes measured: between two flat sheets at an angle, held G0,
//! G1 and G2; between two curved sheets, held G2 at both; and the refusals. And the fillet where
//! two sheets meet: round, chordal and curvature-continuous between two flat sheets at a right
//! angle, the sheets cut back to it or not, and its refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::blend_surface::{
    blend_edges, fillet_sheets, BlendEdges, BlendSide, FilletSheets, Section,
};
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

// ---- fillet between two sheets -------------------------------------------------------------

/// Two flat 10 by 10 sheets meeting at a right angle along the y axis: the first in z = 0 for x
/// in 0..10, the second in x = 0 for z in 0..10.
fn corner() -> (Body, Body) {
    let (a, _) = sheet(
        line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        line([10.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
    );
    let (b, _) = sheet(
        line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
        line([0.0, 0.0, 10.0], [0.0, 10.0, 10.0]),
    );
    (a, b)
}

fn fillet_how(section: Section, trim: bool) -> FilletSheets {
    FilletSheets {
        section,
        trim,
        tolerance: 1e-6,
        stations: 9,
        seam: tight(),
    }
}

/// Whether `body` has an edge through `p`.
fn has_edge_through(body: &Body, p: [f64; 3]) -> bool {
    let (_, order) = body.to_record().unwrap();
    (0..order.edges.len()).any(|i| {
        body.edge_closest_point(&order, i, p)
            .unwrap()
            .is_some_and(|at| at.distance < 1e-6)
    })
}

/// A quarter of a cylinder of radius 5, 10 long: its area.
fn quarter_cylinder() -> f64 {
    std::f64::consts::PI * 5.0 / 2.0 * 10.0
}

/// A round of radius 5 in the right angle touches each sheet 5 from the corner: its contact
/// lines are x = 5 on the first and z = 5 on the second, and it is a quarter cylinder, tangent
/// to both. Not trimmed, the sheets come back as they were (none made).
#[test]
fn a_round_of_radius_5_between_sheets_at_a_right_angle_touches_each_5_from_the_corner() {
    let (a, b) = corner();
    let f = fillet_sheets(&a, &b, &fillet_how(Section::Radius(5.0), false)).unwrap();
    assert_eq!(f.fillet.counts().unwrap()[0], 1);
    assert!(
        has_edge_through(&f.fillet, [5.0, 5.0, 0.0]),
        "contact on the first"
    );
    assert!(
        has_edge_through(&f.fillet, [0.0, 5.0, 5.0]),
        "contact on the second"
    );
    let area = f.fillet.area(1e-10).unwrap();
    assert!((area - quarter_cylinder()).abs() <= 1e-6, "{area}");
    assert!(f.trimmed.is_none());
    assert_eq!(f.seams.len(), 2, "{:?}", f.seams);
    for seam in &f.seams {
        holds(seam, 1);
    }
    assert!(
        (a.area(1e-10).unwrap() - 100.0).abs() <= 1e-9,
        "the first sheet unchanged"
    );
}

/// A chordal round whose chord is 5 sqrt 2 across the right angle is the radius 5 round.
#[test]
fn a_chordal_fillet_of_chord_5_root_2_is_the_radius_5_round() {
    let (a, b) = corner();
    let chord = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let f = fillet_sheets(&a, &b, &fillet_how(Section::Chord(chord), false)).unwrap();
    assert!(has_edge_through(&f.fillet, [5.0, 5.0, 0.0]));
    assert!(has_edge_through(&f.fillet, [0.0, 5.0, 5.0]));
    let area = f.fillet.area(1e-10).unwrap();
    assert!((area - quarter_cylinder()).abs() <= 1e-6, "{area}");
}

/// The curvature-continuous fillet meets both sheets G2 at every station of its seams.
#[test]
fn a_g2_fillet_is_curvature_continuous_with_both_sheets() {
    let (a, b) = corner();
    let f = fillet_sheets(&a, &b, &fillet_how(Section::Curvature(5.0), true)).unwrap();
    assert_eq!(f.fillet.counts().unwrap()[0], 1);
    assert_eq!(f.seams.len(), 2, "{:?}", f.seams);
    for seam in &f.seams {
        holds(seam, 2);
    }
}

/// Trimmed, each sheet comes back cut along its contact line: 10 by 5, from x = 5 (z = 5) out.
#[test]
fn trimmed_the_sheets_are_cut_back_to_the_contact_lines() {
    let (a, b) = corner();
    let f = fillet_sheets(&a, &b, &fillet_how(Section::Radius(5.0), true)).unwrap();
    let (ta, tb) = f.trimmed.unwrap();
    for (t, contact) in [(&ta, [5.0, 5.0, 0.0]), (&tb, [0.0, 5.0, 5.0])] {
        let area = t.area(1e-10).unwrap();
        assert!((area - 50.0).abs() <= 1e-6, "{area}");
        assert!(has_edge_through(t, contact));
    }
    assert!(
        has_edge_through(&ta, [10.0, 5.0, 0.0]),
        "the first keeps its far edge"
    );
    assert!(
        has_edge_through(&tb, [0.0, 5.0, 10.0]),
        "the second keeps its far edge"
    );
}

/// The refusals: a radius the sheets have no room for, sizes and tolerances out of range, one
/// sheet twice, and sheets that do not meet.
#[test]
fn malformed_fillets_are_refused() {
    let (a, b) = corner();
    assert!(fillet_sheets(&a, &b, &fillet_how(Section::Radius(20.0), false)).is_err());
    for bad in [
        fillet_how(Section::Radius(0.0), false),
        fillet_how(Section::Chord(f64::NAN), false),
        FilletSheets {
            tolerance: 0.0,
            ..fillet_how(Section::Radius(5.0), false)
        },
        FilletSheets {
            stations: 1,
            ..fillet_how(Section::Radius(5.0), false)
        },
    ] {
        assert_eq!(
            fillet_sheets(&a, &b, &bad).err(),
            Some(Error::Argument),
            "{bad:?}"
        );
    }
    assert_eq!(
        fillet_sheets(&a, &a, &fillet_how(Section::Radius(5.0), false)).err(),
        Some(Error::Argument)
    );
    let (apart, _) = sheet(
        line([30.0, 0.0, 0.0], [30.0, 10.0, 0.0]),
        line([30.0, 0.0, 10.0], [30.0, 10.0, 10.0]),
    );
    assert!(matches!(
        fillet_sheets(&a, &apart, &fillet_how(Section::Radius(5.0), false)),
        Err(Error::Unsupported(_))
    ));
}
