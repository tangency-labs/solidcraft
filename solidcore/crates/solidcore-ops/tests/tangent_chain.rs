//! `blend::tangent_chain`, `tangent::tangent_face_chain_ordered` (#3141) and
//! `tangent::curve_chain_ordered`, on record order.
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

use solidcore_ops::blend::tangent_chain;
use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::tangent::{curve_chain_ordered, tangent_face_chain_ordered};
use solidcore_ops::{make_box, Body};
use solidcore_record::brep::{Brep as Record, Surface};
use std::f64::consts::{FRAC_PI_2, PI};

const H: f64 = 4.0;
/// SolidCraft's smooth-edge angle, the caller's: half a degree.
const ANGLE: f64 = 0.5 * PI / 180.0;

fn stadium_profile() -> [Curve; 4] {
    [
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
    ]
}

fn stadium() -> Body {
    let profile = stadium_profile();
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
    let chain = tangent_chain(&b, Some(&order), &[front], &[], ANGLE).unwrap();
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
    let chain = tangent_chain(&b, Some(&order), &[front], &[back], ANGLE).unwrap();
    assert_eq!(chain.members.len(), 3, "{chain:?}");
    assert!(!chain.members.contains(&back));
}

#[test]
fn a_box_edge_chains_alone() {
    let b = make_box([0.0; 3], [10.0, 6.0, H]).unwrap();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [10.0, 0.0, H]);
    let chain = tangent_chain(&b, Some(&order), &[front], &[], ANGLE).unwrap();
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
    boolean(base, stadium_boss(), BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// The 50 x 10 stadium boss about y = 0, from z = 10 to 20.
fn stadium_boss() -> Body {
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
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap()
}

#[test]
fn one_pick_follows_the_boss_rim_at_any_part_size() {
    for plate in [140.0, 145.0, 250.0] {
        let b = plate_with_stadium(plate);
        let (r, order) = b.to_record().unwrap();
        let back = edge(&r, [20.0, 5.0, 20.0], [-20.0, 5.0, 20.0]);
        let chain = tangent_chain(&b, Some(&order), &[back], &[], ANGLE).unwrap();
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

/// A prism on (0, 0), (10, 0), (20, 10 tan a), (20, 12), (0, 12), `a` 0.3 degrees: its walls
/// at (10, 0) meet `a` apart. A pick of the wall on y = 0 joins the next wall at half a degree,
/// not at 0.2 degrees, and at 0.3 degrees, the angle itself, the kernel decides neither: the
/// edge between them is listed undecided and joins nothing.
#[test]
fn walls_at_the_asked_angle_are_listed_undecided() {
    // 10 tan(0.3 degrees), as a literal: ops tests use no float methods
    let rise = 0.052_360_356_057_001_275;
    let pts = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [20.0, rise, 0.0],
        [20.0, 12.0, 0.0],
        [0.0, 12.0, 0.0],
    ];
    let profile: Vec<Curve> = (0..5)
        .map(|i| Curve::Line {
            start: pts[i],
            end: pts[(i + 1) % 5],
        })
        .collect();
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        H,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let (r, order) = b.to_record().unwrap();
    let front: Vec<usize> = plane_along(&r, [0.0, 1.0, 0.0])
        .into_iter()
        .filter(|&f| match r.faces[f].surface {
            Surface::Plane { origin, .. } => origin[1].abs() < 1e-9,
            _ => false,
        })
        .collect();
    assert_eq!(front.len(), 1, "{front:?}");
    let kink = edge(&r, [10.0, 0.0, 0.0], [10.0, 0.0, H]);
    // 0.5, 0.2 and 0.3 degrees in radians
    let (half, fifth, level) = (
        0.008_726_646_259_971_648,
        0.003_490_658_503_988_659,
        0.005_235_987_755_982_988,
    );
    let at = |angle: f64| tangent_face_chain_ordered(&b, &order, &front, angle).unwrap();
    let wide = at(half);
    assert_eq!(wide.members.len(), 2, "{wide:?}");
    assert!(wide.undecided.is_empty(), "{wide:?}");
    let narrow = at(fifth);
    assert_eq!(narrow.members, front, "{narrow:?}");
    assert!(narrow.undecided.is_empty(), "{narrow:?}");
    let level = at(level);
    assert_eq!(level.members, front, "{level:?}");
    assert_eq!(level.undecided, vec![kink], "{level:?}");
}

/// The stadium as a sheet (extruded without caps): its top edges have one face each, so a pick
/// of one chains nothing more: an edge not between two faces does not join.
#[test]
fn a_sheet_s_one_face_edges_do_not_join() {
    let profile = stadium_profile();
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        H,
        0.0,
        DraftCorner::default(),
        false,
    )
    .unwrap();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [20.0, 0.0, H]);
    let chain = tangent_chain(&b, Some(&order), &[front], &[], ANGLE).unwrap();
    assert_eq!(chain.members, vec![front], "{chain:?}");
    assert!(chain.undecided.is_empty(), "{chain:?}");
}

/// A plate 80 x 40 x 10 at y >= 0 with the 50 x 10 stadium boss, 10 tall, on its top about
/// y = 0, so the boss's front half overhangs the plate's front face. The boss's base outline at
/// z = 10 is concave where it stands on the plate (the back straight and the back halves of its
/// end arcs) and convex where it overhangs (the boss's underside meets its wall). At
/// (+-25, 0, 10) each end arc's back quarter runs on tangentially into its front quarter, of
/// the other convexity: a pick of the back straight chains the back straight and the two back
/// quarters, and stops there.
#[test]
fn a_chain_stops_where_it_would_turn_over() {
    let base = make_box([-40.0, 0.0, 0.0], [80.0, 40.0, 10.0]).unwrap();
    let b = boolean(base, stadium_boss(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    let (r, order) = b.to_record().unwrap();
    let back = edge(&r, [20.0, 5.0, 10.0], [-20.0, 5.0, 10.0]);
    let chain = tangent_chain(&b, Some(&order), &[back], &[], ANGLE).unwrap();
    assert!(chain.undecided.is_empty(), "{chain:?}");
    assert_eq!(chain.members.len(), 3, "{chain:?}");
    for &e in &chain.members {
        for v in r.edges[e].vertices {
            let q = r.vertices[v as usize].point;
            assert!((q[2] - 10.0).abs() < 1e-9 && q[1] > -1e-9, "{e}: {q:?}");
        }
    }
    let front = edge(&r, [-20.0, -5.0, 10.0], [20.0, -5.0, 10.0]);
    assert!(!chain.members.contains(&front), "{chain:?}");
}

/// Curve chains (`curve_chain_ordered`): the stadium's top outline is one chain as a blend's
/// is, and a box's edge chains alone.
#[test]
fn a_curve_chain_follows_the_stadium_s_top_outline() {
    let b = stadium();
    let (r, order) = b.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [20.0, 0.0, H]);
    let chain = curve_chain_ordered(&b, &order, &[front], ANGLE).unwrap();
    assert_eq!(chain.members.len(), 4, "{chain:?}");
    assert_eq!(chain.members[0], front);
    assert!(chain.members.iter().all(|&e| on_top(&r, e)), "{chain:?}");
    let square = make_box([0.0; 3], [10.0, 6.0, H]).unwrap();
    let (r, order) = square.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [10.0, 0.0, H]);
    let chain = curve_chain_ordered(&square, &order, &[front], ANGLE).unwrap();
    assert_eq!(chain.members, vec![front]);
}

/// A curve chain does not care for the faces: the sheet stadium's one-face top edges chain
/// (a blend's chain does not, `a_sheet_s_one_face_edges_do_not_join`), and so does the
/// overhanging boss's whole base outline, concave and convex (a blend's chain stops where it
/// would turn over, `a_chain_stops_where_it_would_turn_over`).
#[test]
fn a_curve_chain_runs_on_where_a_blend_s_chain_stops() {
    let sheet = extrude(
        &stadium_profile(),
        [0.0, 0.0, 1.0],
        H,
        0.0,
        DraftCorner::default(),
        false,
    )
    .unwrap();
    let (r, order) = sheet.to_record().unwrap();
    let front = edge(&r, [0.0, 0.0, H], [20.0, 0.0, H]);
    let chain = curve_chain_ordered(&sheet, &order, &[front], ANGLE).unwrap();
    assert_eq!(chain.members.len(), 4, "{chain:?}");
    assert!(chain.members.iter().all(|&e| on_top(&r, e)), "{chain:?}");

    let base = make_box([-40.0, 0.0, 0.0], [80.0, 40.0, 10.0]).unwrap();
    let b = boolean(base, stadium_boss(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    let (r, order) = b.to_record().unwrap();
    let back = edge(&r, [20.0, 5.0, 10.0], [-20.0, 5.0, 10.0]);
    let front = edge(&r, [-20.0, -5.0, 10.0], [20.0, -5.0, 10.0]);
    let chain = curve_chain_ordered(&b, &order, &[back], ANGLE).unwrap();
    assert!(chain.members.contains(&front), "{chain:?}");
    for &e in &chain.members {
        for v in r.edges[e].vertices {
            let q = r.vertices[v as usize].point;
            assert!((q[2] - 10.0).abs() < 1e-9, "{e}: {q:?}");
        }
    }
}

/// Hostile picks are refused, never a panic: none, an index past the edges, a bad angle.
#[test]
fn a_curve_chain_refuses_bad_picks() {
    let b = stadium();
    let (_, order) = b.to_record().unwrap();
    assert!(curve_chain_ordered(&b, &order, &[], ANGLE).is_err());
    assert!(curve_chain_ordered(&b, &order, &[usize::MAX], ANGLE).is_err());
    assert!(curve_chain_ordered(&b, &order, &[0], f64::NAN).is_err());
    assert!(curve_chain_ordered(&b, &order, &[0], 2.0).is_err());
}
