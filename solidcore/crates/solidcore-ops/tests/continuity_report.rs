//! #3142: the edge-continuity check and edge combs through ops, on record order, against closed
//! forms: a box's edges are G0 right angles; a round of radius r between two of its faces meets
//! each tangentially (G1), its curvature jumping by 1 / r; a cylinder's circles have teeth of
//! length 1 / R pointing at the axis.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::edge_at;
use solidcore_ops::continuity::{ContinuityTolerances, G1AllAlong};
use solidcore_ops::{blend, make_box, make_cylinder, Blend, Corners};

/// SolidCraft's tolerances, the caller's: a micron, a degree, 1/100 mm and a tenth of that
/// again per millimetre twice.
fn tolerances() -> ContinuityTolerances {
    ContinuityTolerances {
        gap_mm: 1e-3,
        angle_rad: std::f64::consts::PI / 180.0,
        curvature_per_mm: 1e-2,
        curvature_change_per_mm2: 1e-3,
        curvature_change_rate_per_mm3: 1e-4,
    }
}

#[test]
fn a_box_s_edges_are_right_angles_and_g0_only() {
    let b = make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let (_, order) = b.to_record().unwrap();
    let r = b.continuity_report(&order, &[], 9, tolerances()).unwrap();
    assert_eq!(r.seams.len(), 12);
    assert!(r.unmeasured.is_empty());
    for s in &r.seams {
        assert_eq!(s.stations.len(), 9, "edge {}", s.edge);
        for st in &s.stations {
            assert!(st.gap_mm <= 1e-12, "{st:?}");
            assert!((st.normal_angle_deg - 90.0).abs() <= 1e-9, "{st:?}");
        }
        assert_eq!(s.order_at_stations, Some(0), "a crease: G0 only");
        assert_eq!(s.g1_all_along, G1AllAlong::NotTangent, "edge {}", s.edge);
    }
}

#[test]
fn a_round_s_rails_are_tangent_with_a_curvature_jump_of_its_inverse_radius() {
    let b = make_box([0.0, 0.0, 0.0], [40.0, 30.0, 20.0]).unwrap();
    let r = 4.0;
    let e = edge_at(&b, [20.0, 30.0, 20.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let (_, order) = b.to_record().unwrap();
    let report = b.continuity_report(&order, &[], 9, tolerances()).unwrap();
    let tangent: Vec<_> = report
        .seams
        .iter()
        .filter(|s| s.order_at_stations == Some(1))
        .collect();
    assert_eq!(tangent.len(), 2, "the round's two rails: {report:?}");
    for s in tangent {
        assert_eq!(
            s.g1_all_along,
            G1AllAlong::Tangent,
            "a rail, proved all along"
        );
        for st in &s.stations {
            assert!(st.normal_angle_deg <= 1e-4, "{st:?}");
            assert!((st.curvature_jump_per_mm - 1.0 / r).abs() <= 1e-6, "{st:?}");
        }
    }
    // every other seam is a crease
    assert!(report
        .seams
        .iter()
        .all(|s| s.order_at_stations == Some(1) || s.order_at_stations == Some(0)));
}

#[test]
fn a_cylinder_s_circles_comb_towards_the_axis() {
    let radius = 7.0;
    let b = make_cylinder([0.0, 0.0, 0.0], radius, 12.0).unwrap();
    let (_, order) = b.to_record().unwrap();
    let mut circles = 0;
    for edge in 0..order.edges.len() {
        let teeth = b.edge_comb(&order, edge, 17).unwrap();
        let bent: Vec<_> = teeth
            .iter()
            .filter(|t| t.curvature.iter().any(|c| c.abs() > 1e-12))
            .collect();
        if bent.is_empty() {
            continue;
        }
        circles += 1;
        assert_eq!(
            bent.len(),
            17,
            "a closed circle: 17 teeth, its end its start once"
        );
        for t in bent {
            let [x, y, z] = t.curvature;
            let len2 = x * x + y * y + z * z;
            assert!((len2 * radius * radius - 1.0).abs() <= 1e-12, "{t:?}");
            // towards the axis: the tooth's xy points against the point's
            let dot = x * t.at[0] + y * t.at[1];
            assert!((dot + 1.0).abs() <= 1e-9, "{t:?}");
        }
    }
    assert_eq!(circles, 2, "the top and bottom circles");
}
