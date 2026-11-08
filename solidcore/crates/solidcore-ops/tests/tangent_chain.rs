//! `tangent::tangent_chain_ordered` and `tangent_face_chain_ordered` (#3141), on record order.
//!
//! A stadium prism: two straight walls and two half-cylinders, 4 high. Its top outline is
//! four edges each running on tangentially into the next, so a pick of one top edge chains
//! all four and nothing else (the walls meet each other smoothly, so the vertical edges
//! between them are smooth, and the bottom outline turns square from the top's). Its walls
//! meet across smooth edges, so a pick of one wall chains the four walls and not the caps. A
//! box's edges and faces all meet square: a pick chains itself alone.
//!
//! Ported from SolidCraft (`crates/kernel/tests/chamfer_curved_holder.rs`,
//! `one_chamfer_pick_follows_the_tangent_chain_at_any_part_size`, its #2672): a plate of side
//! 140, 145 and 250, 10 thick, with a 50 x 10 stadium boss 10 tall on top. One pick on the
//! boss's top straight edge chains the boss's whole top rim, its four edges at z = 20, at
//! every plate size (SolidCraft's walk once stopped at the straight edge past a 200 mm part).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::tangent::{tangent_chain_ordered, tangent_face_chain_ordered};
use solidcore_ops::{make_box, Body};
use solidcore_record::brep::{Brep as Record, Surface};
use std::f64::consts::{FRAC_PI_2, PI};

const H: f64 = 4.0;
/// SolidCraft's smooth-edge angle, the caller's: half a degree.
const ANGLE: f64 = 0.5 * PI / 180.0;

fn stadium() -> Body {
    let profile = [
        Curve::Line {
            start: [0.0, 0.0, 0.0],
            end: [20.0, 0.0, 0.0],
        },
        Curve::Arc {
            center: [20.0, 5.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: -FRAC_PI_2,
            end: FRAC_PI_2,
        },
        Curve::Line {
            start: [20.0, 10.0, 0.0],
            end: [0.0, 10.0, 0.0],
        },
        Curve::Arc {
            center: [0.0, 5.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: FRAC_PI_2,
            end: 1.5 * PI,
        },
    ];
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        H,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap()
}

/// The record index of the edge between the vertices at `a` and `b`.
fn edge(r: &Record, a: [f64; 3], b: [f64; 3]) -> usize {
    let near = |i: u32, p: [f64; 3]| {
        let q = r.vertices[i as usize].point;
        (0..3).all(|j| (q[j] - p[j]).abs() < 1e-9)
    };
    r.edges
        .iter()
        .position(|e| {
            let [s, t] = e.vertices;
            (near(s, a) && near(t, b)) || (near(s, b) && near(t, a))
        })
        .expect("an edge between the points")
}

/// Whether record edge `i` lies in the top, both its ends at z = H.
fn on_top(r: &Record, i: usize) -> bool {
    r.edges[i]
        .vertices
        .iter()
        .all(|&v| (r.vertices[v as usize].point[2] - H).abs() < 1e-9)
}

/// The record index of the planar face whose normal is along `n` (up to sign).
fn plane_along(r: &Record, n: [f64; 3]) -> Vec<usize> {
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let along = |m: [f64; 3]| {
        let c = cross(m, n);
        c.iter().all(|x| x.abs() < 1e-9) && m.iter().any(|x| x.abs() > 1e-9)
    };
    (0..r.faces.len())
        .filter(|&i| match r.faces[i].surface {
            Surface::Plane { u_axis, v_axis, .. } => along(cross(u_axis, v_axis)),
            _ => false,
        })
        .collect()
}

#[test]
fn a_stadium_s_top_outline_is_one_chain() {
    let b = stadium();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [20.0, 0.0, H]);
    let chain = tangent_chain_ordered(&b, &order, &[front], &[], ANGLE).unwrap();
    assert!(chain.undecided.is_empty(), "{chain:?}");
    assert_eq!(chain.members.len(), 4, "{chain:?}");
    assert_eq!(chain.members[0], front);
    assert!(chain.members.iter().all(|&e| on_top(&r, e)), "{chain:?}");
}

#[test]
fn a_passed_over_edge_stops_the_chain() {
    let b = stadium();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [20.0, 0.0, H]);
    let back = edge(&r, [20.0, 10.0, H], [0.0, 10.0, H]);
    let chain = tangent_chain_ordered(&b, &order, &[front], &[back], ANGLE).unwrap();
    assert_eq!(chain.members.len(), 3, "{chain:?}");
    assert!(!chain.members.contains(&back));
}

#[test]
fn a_box_edge_chains_alone() {
    let b = make_box([0.0; 3], [10.0, 6.0, H]).unwrap();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [10.0, 0.0, H]);
    let chain = tangent_chain_ordered(&b, &order, &[front], &[], ANGLE).unwrap();
    assert_eq!(chain.members, vec![front]);
    assert!(chain.undecided.is_empty());
}

#[test]
fn a_stadium_s_walls_are_one_face_chain() {
    let b = stadium();
    let (r, order) = b.to_record().unwrap();
    let walls = plane_along(&r, [0.0, 1.0, 0.0]);
    assert_eq!(walls.len(), 2);
    let chain = tangent_face_chain_ordered(&b, &order, &walls[..1], ANGLE).unwrap();
    assert!(chain.undecided.is_empty(), "{chain:?}");
    assert_eq!(chain.members.len(), 4, "{chain:?}");
    for &f in &chain.members {
        let caps = plane_along(&r, [0.0, 0.0, 1.0]);
        assert!(!caps.contains(&f), "a cap joined: {chain:?}");
    }
    assert!(walls.iter().all(|w| chain.members.contains(w)), "{chain:?}");
}

#[test]
fn a_box_face_chains_alone() {
    let b = make_box([0.0; 3], [10.0, 6.0, H]).unwrap();
    let (r, order) = b.to_record().unwrap();
    let top = plane_along(&r, [0.0, 0.0, 1.0]);
    let chain = tangent_face_chain_ordered(&b, &order, &top[..1], ANGLE).unwrap();
    assert_eq!(chain.members, vec![top[0]]);
}

/// The plate of side `plate` with the stadium boss on top.
fn plate_with_stadium(plate: f64) -> Body {
    let base = make_box([-plate / 2.0, -plate / 2.0, 0.0], [plate, plate, 10.0]).unwrap();
    let z = 10.0;
    let profile = [
        Curve::Line {
            start: [-20.0, -5.0, z],
            end: [20.0, -5.0, z],
        },
        Curve::Arc {
            center: [20.0, 0.0, z],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: -FRAC_PI_2,
            end: FRAC_PI_2,
        },
        Curve::Line {
            start: [20.0, 5.0, z],
            end: [-20.0, 5.0, z],
        },
        Curve::Arc {
            center: [-20.0, 0.0, z],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: FRAC_PI_2,
            end: 1.5 * PI,
        },
    ];
    let boss = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    boolean(base, boss, BooleanOp::Union).unwrap().unwrap()
}

#[test]
fn one_pick_follows_the_boss_rim_at_any_part_size() {
    for plate in [140.0, 145.0, 250.0] {
        let b = plate_with_stadium(plate);
        let (r, order) = b.to_record().unwrap();
        let back = edge(&r, [20.0, 5.0, 20.0], [-20.0, 5.0, 20.0]);
        let chain = tangent_chain_ordered(&b, &order, &[back], &[], ANGLE).unwrap();
        assert!(chain.undecided.is_empty(), "plate {plate}: {chain:?}");
        assert_eq!(chain.members.len(), 4, "plate {plate}: {chain:?}");
        let at_top = |i: usize| {
            r.edges[i]
                .vertices
                .iter()
                .all(|&v| (r.vertices[v as usize].point[2] - 20.0).abs() < 1e-9)
        };
        assert!(
            chain.members.iter().all(|&e| at_top(e)),
            "plate {plate}: {chain:?}"
        );
    }
}
