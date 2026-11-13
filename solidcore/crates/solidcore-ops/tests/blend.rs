//! Fillets and chamfers against closed forms (the cases of SolidCraft's `core/blend_tests.rs`,
//! on edge, face and vertex indices). Their own test binary: each holds the process kernel for
//! seconds.
#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used
)] // test helpers

mod common;

use common::*;

use std::f64::consts::PI;
use std::io::Read as _;

use solidcore::blend::definition_check::surface_kind_name;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};
use solidcore::surface::procedural_blend::{BlendRef, ProceduralBlend};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::topology::{edge, loopuse};
use solidcore::{EdgeId, FaceId, Point2, Vec3};
use solidcore_record::brep::Brep as Record;

use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{
    blend, blend_ordered, chamfer_sides, edge_faces_along, fillet_variable, make_box, make_cone,
    make_cylinder, tangent_chain, Blend, BlendOptions, Body, ChamferSecond, CheckLevel, Corners,
    Error, FitKind, Order, SetBack, Turn, Unblendable,
};

/// The block's top front edge, 30 long, between the top (z = 10) and the front (y = 0).
fn top_front(b: &Body) -> usize {
    edge_at(b, [15.0, 0.0, 10.0])
}

#[test]
fn rounds_and_chamfers_take_their_closed_forms() {
    all_fixes();
    // a round of r 2 along 30: (1 - pi/4) r^2 off per unit length
    let b = block();
    let e = top_front(&b);
    let made = blend(&b, Blend::Round, &[e], &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(made.dropped, vec![false]);
    assert!(made.holes.is_empty());
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-9);
    // an equal chamfer of 2 along 30: d^2 / 2 off per unit length
    let b = block();
    blend(
        &b,
        Blend::Chamfer,
        &[top_front(&b)],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - 2.0 * 30.0) < 1e-9);
    // a chord round of width 2 sqrt 2 between planes at 90 degrees is the round of r 2
    let b = block();
    blend(
        &b,
        Blend::Chord,
        &[top_front(&b)],
        &[2.0 * std::f64::consts::SQRT_2],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-8);
    // the curvature round builds and takes off the ball's area
    let b = block();
    blend(
        &b,
        Blend::Curvature,
        &[top_front(&b)],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-6);
}

#[test]
fn three_rounds_meet_at_a_corner_filled_set_back_or_open() {
    all_fixes();
    let edges = |b: &Body| {
        [
            edge_at(b, [15.0, 20.0, 10.0]),
            edge_at(b, [30.0, 10.0, 10.0]),
            edge_at(b, [30.0, 20.0, 5.0]),
        ]
    };
    // the ball's corner
    let b = block();
    blend(&b, Blend::Round, &edges(&b), &[2.0], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let ball = volume(&b);
    // set back 3 at the corner: a different corner, a closed solid
    let b = block();
    let corner = vertex_at(&b, [30.0, 20.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 3.0,
    }];
    blend(&b, Blend::Round, &edges(&b), &[2.0], &back, Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(rel(volume(&b), ball) < 1e-3);
    // opened: the corner's hole, its edges closing one loop, each on a face outside it
    let b = block();
    let corner = vertex_at(&b, [30.0, 20.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 3.0,
    }];
    let made = blend(&b, Blend::Round, &edges(&b), &[2.0], &back, Corners::Open).unwrap();
    assert_eq!(made.holes.len(), 1);
    let [faces, edges_n, _] = b.counts().unwrap();
    for h in &made.holes[0].edges {
        assert!(h.edge < edges_n && h.face < faces);
    }
    assert!(made.holes[0].edges.len() >= 3);
}

/// The record order of `b`, and the record index of each edge of the loop through `corners`
/// (x, y) at height `z`, in the loop's order.
fn loop_edges(b: &Body, corners: [[f64; 2]; 4], z: f64) -> (Order, Vec<usize>) {
    let (record, order) = b.to_record().unwrap();
    let at = |i: u32, c: [f64; 2]| {
        let p = record.vertices[i as usize].point;
        (p[0] - c[0]).abs() + (p[1] - c[1]).abs() + (p[2] - z).abs() < 1e-9
    };
    let edges = (0..4)
        .map(|i| {
            let (a, c) = (corners[i], corners[(i + 1) % 4]);
            record
                .edges
                .iter()
                .position(|e| {
                    let [s, t] = e.vertices;
                    (at(s, a) && at(t, c)) || (at(s, c) && at(t, a))
                })
                .unwrap()
        })
        .collect();
    (order, edges)
}

/// A plate 2 thick whose top outline runs 50 along x and then 40 at a shallow angle off it,
/// turning `kink` degrees (towards the plate for `convex`, away from it else), its other sides
/// square to those two: the two top edges rounded r 1 or chamfered 1 turn round the sharp
/// upright edge between them, mitred. Each edge loses its section's area A along its length
/// (90 in all), and the mitre on the bisector takes the corner's part off one side and adds it
/// on the other: the section's first moment M about the upright edge times tan(kink / 2), twice,
/// off at the convex corner and on at the concave one. A round's A = (1 - pi/4) r^2 and
/// M = (5/6 - pi/4) r^3; a chamfer's A = d^2 / 2 and M = d^3 / 6. Edges a few degrees off
/// tangent used to be read as continuing each other (within the solvers' 10 degree tangency
/// tolerance) and given one shared section: the two balls stand the radius times the angle
/// apart there, the trim leaked and the blend failed with status 1001.
#[test]
fn blends_turn_round_a_shallow_sharp_corner() {
    all_fixes();
    let plate = |kink: f64, convex: bool| {
        let t = if convex { -kink } else { kink }.to_radians();
        let (u, n) = ([t.cos(), t.sin()], [t.sin(), -t.cos()]);
        let p = [
            [0.0, 0.0],
            [50.0, 0.0],
            [50.0 + 40.0 * u[0], 40.0 * u[1]],
            [50.0 + 40.0 * u[0] + 30.0 * n[0], 40.0 * u[1] + 30.0 * n[1]],
            [0.0, -30.0],
        ];
        let profile: Vec<Curve> = (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                Curve::Line {
                    start: [a[0], a[1], 0.0],
                    end: [b[0], b[1], 0.0],
                }
            })
            .collect();
        let b = extrude(
            &profile,
            [0.0, 0.0, 1.0],
            2.0,
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap();
        let edges = [
            edge_at(&b, [25.0, 0.0, 2.0]),
            edge_at(&b, [50.0 + 20.0 * u[0], 20.0 * u[1], 2.0]),
        ];
        (b, edges)
    };
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [0.5, 1.5, 3.0, 6.0] {
        for convex in [true, false] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                let corner = 2.0 * m * (kink.to_radians() / 2.0).tan();
                let want = 90.0 * a + if convex { -corner } else { corner };
                let off = before - volume(&b);
                assert!(
                    rel(off, want) < 1e-6,
                    "{kind:?} at a {kink} degree corner (convex {convex}): {off} off, want {want}"
                );
            }
        }
    }
}

/// A 40 x 30 x 10 plate with a 10 x 10 x 5 boss on it (x 15..25, y 10..20), the boss's four
/// base edges rounded r 1: at each upright boss edge two concave rounds turn round the sharp
/// convex edge. Each base edge gains the spandrel A = (1 - pi/4) r^2 along its 10, and each
/// corner the rounds' part past both walls, whose section's first moment about the upright edge
/// is M = (5/6 - pi/4) r^3: mitred, 8 M in all; swept, the section revolved a quarter turn at
/// each, 2 pi M, its face a torus. Unequal rounds are not swept yet. Along a pocket's floor the
/// upright edges are sharp and concave: no ball turns round them, so the swept turn is refused
/// there and the mitre builds, 60 A - 8 M. A swept turn is a round's.
#[test]
fn rounds_turn_round_a_boss_edge_mitred_or_swept() {
    all_fixes();
    let bx = |k: &mut Kernel, lo: [f64; 3], size: [f64; 3]| {
        create_box(k, Vec3::new(lo[0], lo[1], lo[2]), size[0], size[1], size[2])
    };
    let boss = || {
        built(|k| {
            let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
            let boss = bx(k, [15.0, 10.0, 10.0], [10.0, 10.0, 5.0])?;
            boolean_union(k, plate, boss)
        })
    };
    let pocket = || {
        built(|k| {
            let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
            let cut = bx(k, [10.0, 10.0, 5.0], [20.0, 10.0, 6.0])?;
            boolean_difference(k, plate, cut)
        })
    };
    let base = [[15.0, 10.0], [25.0, 10.0], [25.0, 20.0], [15.0, 20.0]];
    let floor = [[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]];
    let (a, m) = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    assert_eq!(Turn::default(), Turn::Mitre);
    for (turn, corners, faces) in [(Turn::Mitre, 8.0 * m, 15), (Turn::Swept, 2.0 * PI * m, 19)] {
        let b = boss();
        let (order, edges) = loop_edges(&b, base, 10.0);
        blend_ordered(
            &b,
            &order,
            Blend::Round,
            &edges,
            &[1.0],
            &[],
            Corners::Filled,
            BlendOptions {
                turn,
                ..BlendOptions::default()
            },
        )
        .unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{turn:?}");
        assert_eq!(b.counts().unwrap()[0], faces, "{turn:?}");
        let v = b.volume(1e-10).unwrap();
        assert!(rel(v, 12500.0 + 40.0 * a + corners) < 1e-9, "{turn:?}: {v}");
        // swept, the four corners' faces are exact horn tori (R = r = 1), as the kernel reads
        // the body back; mitred, there are none. That each blend face answers
        // procedural_blend() is the kernel's definition ledger
        // (crates/solidcore/tests/issue_2489_blend_faces_are_definitions.rs, the boss base with
        // mitred and swept turns): an ops body reads back through its .smb, which keeps the
        // exact rounds' and corners' forms but not their definitions
        let (_, made) = kernel_copy(&b);
        let tori: Vec<(f64, f64)> = made
            .face_list()
            .iter()
            .filter_map(|&f| match made.face(f).unwrap().surface.as_ref()?.kind() {
                SurfaceKind::Torus(t) => Some((t.major_radius(), t.minor_radius())),
                _ => None,
            })
            .collect();
        let want = if turn == Turn::Swept { 4 } else { 0 };
        assert_eq!(tori, vec![(1.0, 1.0); want], "{turn:?}");
    }
    let swept = |b: &Body, order: &Order, kind, edges: &[usize], sizes: &[f64]| {
        blend_ordered(
            b,
            order,
            kind,
            edges,
            sizes,
            &[],
            Corners::Filled,
            BlendOptions {
                turn: Turn::Swept,
                ..BlendOptions::default()
            },
        )
        .err()
    };
    let b = boss();
    let (order, edges) = loop_edges(&b, base, 10.0);
    let unequal = [1.0, 1.5, 1.0, 1.5];
    let r = swept(&b, &order, Blend::Round, &edges, &unequal);
    assert_eq!(r, Some(Error::CornerUnsupported));
    let r = swept(&b, &order, Blend::Chamfer, &edges, &[1.0]);
    assert_eq!(r, Some(Error::Argument));
    let p = pocket();
    let (order, edges) = loop_edges(&p, floor, 5.0);
    let before = p.to_record().unwrap().0;
    let r = swept(&p, &order, Blend::Round, &edges, &[1.0]);
    assert_eq!(r, Some(Error::NoRoom));
    assert_eq!(p.to_record().unwrap().0, before);
    blend_ordered(
        &p,
        &order,
        Blend::Round,
        &edges,
        &[1.0],
        &[],
        Corners::Filled,
        BlendOptions {
            turn: Turn::Mitre,
            ..BlendOptions::default()
        },
    )
    .unwrap();
    let v = p.volume(1e-10).unwrap();
    assert!(rel(v, 11000.0 + 60.0 * a - 8.0 * m) < 1e-9, "{v}");
}

#[test]
fn chamfers_by_sides_take_the_face_asked() {
    all_fixes();
    // 1 back along the top, 3 down the front: d1 d2 / 2 off per unit length either way round,
    // and the top's leg is 1 when the top is asked first
    for up in [true, false] {
        let b = block();
        let e = top_front(&b);
        let faces = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
        let top = if faces[0].1 > faces[1].1 {
            faces[0].0
        } else {
            faces[1].0
        };
        let front = if faces[0].1 > faces[1].1 {
            faces[1].0
        } else {
            faces[0].0
        };
        let first = if up { top } else { front };
        chamfer_sides(&b, &[e], &[first], 1.0, ChamferSecond::Distance(3.0)).unwrap();
        assert!(rel(volume(&b), ABC - 1.5 * 30.0) < 1e-9);
        // the top keeps its leg: (30 x (20 - 1)) when asked first, else 20 - 3
        let leg = if up { 19.0 } else { 17.0 };
        let (k, made) = kernel_copy(&b);
        let top_area = made
            .face_list()
            .iter()
            .find_map(|&f| {
                let bx = face_bounding_box(&k, &made, f, true).ok()?;
                ((bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9)
                    .then(|| bx.max().y - bx.min().y)
            })
            .unwrap();
        assert!((top_area - leg).abs() < 1e-9, "{top_area}");
    }
    // a 45 degree chamfer 2 back along the top is the equal one
    let b = block();
    let e = top_front(&b);
    let top = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
    let top = if top[0].1 > top[1].1 {
        top[0].0
    } else {
        top[1].0
    };
    chamfer_sides(&b, &[e], &[top], 2.0, ChamferSecond::Angle(PI / 4.0)).unwrap();
    assert!(rel(volume(&b), ABC - 2.0 * 30.0) < 1e-9);
}

#[test]
fn a_variable_round_runs_from_its_start() {
    all_fixes();
    // r from 1 to 2 along the top front edge: between the constant rounds of 1 and 2
    let b = block();
    let e = top_front(&b);
    let start = vertex_at(&b, [0.0, 0.0, 10.0]);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]).unwrap();
    let v = volume(&b);
    let off = |r: f64| (1.0 - PI / 4.0) * r * r * 30.0;
    assert!(v < ABC - off(1.0) && v > ABC - off(2.0), "{v}");
    assert!(b.check(CheckLevel::All).unwrap().is_valid());

    // a law zero inside, one that starts off the edge
    let b = block();
    let e = top_front(&b);
    let start = vertex_at(&b, [0.0, 0.0, 10.0]);
    let pinched = fillet_variable(
        &b,
        e,
        start,
        1,
        &[0.0, 0.0, 0.5, 1.0, 1.0],
        &[1.0, 0.0, 1.0],
    );
    assert!(matches!(pinched, Err(Error::Unsupported(_))), "{pinched:?}");
    let away = vertex_at(&b, [30.0, 20.0, 0.0]);
    let off_edge = fillet_variable(&b, e, away, 1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]);
    assert_eq!(off_edge, Err(Error::Argument));
    // refused: the body as it came in
    assert!(rel(volume(&b), ABC) < 1e-12);
}

/// #14405: a variable round's definition net is certified on wide pieces. Over a piece of
/// the round the touch circle's rows are bounded by Taylor models in s as well as by
/// interval series, which take the rails and the ball's centre (they move together) as
/// independent ranges: with those alone the circle proved only on pieces some hundredths of
/// the round long, and this round took 113 span certificates and 8,592 enclosures (about
/// 10 s at opt 3), its net 23 inner knots along the round; now 3 and 66 (0.6 s), and 1.
/// The net is the balls' envelope within its certified bound wherever sampled (its distance
/// from the law's balls, measured without the certificate: 1.6e-14, the bound 3.2e-6), and
/// the body is sound.
#[test]
fn a_variable_round_s_net_is_certified_on_wide_pieces() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
    let (from, middle) = ([10.0, 0.0, 30.0], [10.0, 10.0, 30.0]);
    let e = edge_at(&b, middle);
    let start = vertex_at(&b, from);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[2.0, 5.0]).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let (_, made) = kernel_copy(&b);
    // the ball of radius 2 at the start touches the faces x = 10 and z = 30
    let balls = LawPieces::new(from, middle, [[10.0, 0.0, 28.0], [8.0, 0.0, 30.0]], 2.0);
    let mut rounds = 0;
    for &f in made.face_list() {
        let s = made.face(f).unwrap().surface.as_ref().unwrap();
        let Some(round @ BlendRef::Procedural(pb)) = s.procedural_blend() else {
            continue;
        };
        rounds += 1;
        let net = pb.net();
        let d = net.natural_uv_domain();
        let (_, _, swapped) = pb.definition_map();
        let nurb = net.gw_nurb().unwrap();
        let (knots, lo, hi) = if swapped {
            (nurb.v_knots(), d.min().y, d.max().y)
        } else {
            (nurb.u_knots(), d.min().x, d.max().x)
        };
        let mut inner: Vec<f64> = knots
            .iter()
            .copied()
            .filter(|&x| x > lo && x < hi)
            .collect();
        inner.dedup();
        let bound = round.bound();
        let mut worst = 0.0_f64;
        for i in 0..=80 {
            for j in 0..=8 {
                let (a, c) = (f64::from(i) / 80.0, f64::from(j) / 8.0);
                let uv = Point2::new(
                    d.min().x + a * (d.max().x - d.min().x),
                    d.min().y + c * (d.max().y - d.min().y),
                );
                let p = net.evaluate_point(uv).unwrap();
                worst = worst.max(balls.off(p, |y| 2.0 + 3.0 * y));
            }
        }
        eprintln!(
            "{} inner knots, bound {bound:e}, off the balls {worst:e}",
            inner.len()
        );
        assert!(inner.len() <= 2, "{} inner knots", inner.len());
        assert!(worst <= bound, "{worst} over {bound}");
    }
    assert_eq!(rounds, 1);
}

/// A law with a corner rounds its edge as the union of its balls (`docs/generic-blend.md` §3.2,
/// "Corners in a radius law"): at a peak the two sides' rounds cross and end on their crossing,
/// at a dip the corner ball's zone lies between them. Each removes its balls' union, integrated
/// numerically (SolidCraft's `variable_radius_with_a_corner_through_a_middle_point` for the
/// peak, the fork's capi cases for the others, as `wr685_law_corners` holds them), to that
/// test's 1e-4 relative; each keeps the law's radius at its start and is one sound body.
#[test]
fn a_variable_round_with_a_corner_in_its_law_is_its_balls_union() {
    // box size, the edge's middle, its start, the start's rail ends, knots, radii, removed, faces
    type Case<'a> = (
        [f64; 3],
        [f64; 3],
        [f64; 3],
        [[f64; 3]; 2],
        &'a [f64],
        &'a [f64],
        f64,
        usize,
    );
    all_fixes();
    let peak = [0.0, 0.0, 0.5, 1.0, 1.0];
    let dip_then_peak = [0.0, 0.0, 0.25, 0.75, 1.0, 1.0];
    let cases: [Case<'_>; 4] = [
        // 2 -> 4 -> 2 on the 10 x 20 x 30 box's top edge x = 10, z = 30, from y = 0
        (
            [10.0, 20.0, 30.0],
            [10.0, 10.0, 30.0],
            [10.0, 0.0, 30.0],
            [[8.0, 0.0, 30.0], [10.0, 0.0, 28.0]],
            &peak,
            &[2.0, 4.0, 2.0],
            39.541034,
            8,
        ),
        // 4 -> 2 -> 4 on the 20 cube's edge y = z = 20, from x = 0
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [0.0, 20.0, 20.0],
            [[0.0, 16.0, 20.0], [0.0, 20.0, 16.0]],
            &peak,
            &[4.0, 2.0, 4.0],
            39.542073,
            9,
        ),
        // 4, 2, 4, 2 at 0, 0.25, 0.75, 1 on the same edge, from either end (mirror images)
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [0.0, 20.0, 20.0],
            [[0.0, 16.0, 20.0], [0.0, 20.0, 16.0]],
            &dip_then_peak,
            &[4.0, 2.0, 4.0, 2.0],
            38.818277,
            10,
        ),
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [20.0, 20.0, 20.0],
            [[20.0, 16.0, 20.0], [20.0, 20.0, 16.0]],
            &dip_then_peak,
            &[4.0, 2.0, 4.0, 2.0],
            38.818277,
            10,
        ),
    ];
    for (size, middle, from, rails, knots, radii, want, faces) in cases {
        let b = make_box([0.0; 3], size).unwrap();
        let e = edge_at(&b, middle);
        let start = vertex_at(&b, from);
        fillet_variable(&b, e, start, 1, knots, radii).unwrap();
        let removed = size[0] * size[1] * size[2] - volume(&b);
        eprintln!("{radii:?} from {from:?}: removed {removed}, want {want}");
        assert!(
            (removed - want).abs() <= 1e-4 * want,
            "{radii:?} from {from:?}: removed {removed}, want {want}"
        );
        assert_eq!(b.counts().unwrap()[0], faces, "{radii:?} from {from:?}");
        // the round's end at its start is a section of the law's first radius
        for p in rails {
            vertex_at(&b, p);
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        // every face the split makes is the round's definition, not a trace of it: each piece
        // between the law's corners a rolling-ball round of its own piece of the law, and each
        // dip's zone the corner ball's patch
        let (k, made) = kernel_copy(&b);
        let corners: Vec<f64> = knots
            .windows(2)
            .filter(|w| w[0] != w[1])
            .map(|w| w[0])
            .collect();
        let corners = [corners.as_slice(), &[1.0]].concat();
        let dips = (1..radii.len() - 1)
            .filter(|&i| radii[i] < radii[i - 1] && radii[i] < radii[i + 1])
            .count();
        let piece_round = LawPieces::new(from, middle, rails, radii[0]);
        let (mut pieces, mut zones) = (Vec::new(), 0);
        for &f in made.face_list() {
            let s = made.face(f).unwrap().surface.as_ref().unwrap();
            match s.procedural_blend() {
                _ if matches!(s.kind(), SurfaceKind::Plane(_)) => {}
                Some(BlendRef::Procedural(pb)) => {
                    // the piece whose law every point of the definition rolls
                    let law = (0..radii.len() - 1).find(|&i| {
                        let r = |y: f64| {
                            let c = (y - corners[i]) / (corners[i + 1] - corners[i]);
                            radii[i] + c * (radii[i + 1] - radii[i])
                        };
                        definition_points(&k, pb)
                            .iter()
                            .all(|&p| piece_round.off(p, r) < 1e-6)
                    });
                    pieces.push(law);
                }
                // (the copy keeps the zone's ball patch, not the corner definition beside it)
                None if surface_kind_name(s) == "ball-patch" => zones += 1,
                _ => panic!("{radii:?} from {from:?}: {:?}", surface_kind_name(s)),
            }
        }
        pieces.sort_unstable();
        let want: Vec<Option<usize>> = (0..radii.len() - 1).map(Some).collect();
        assert_eq!(pieces, want, "{radii:?} from {from:?}");
        assert_eq!(zones, dips, "{radii:?} from {from:?}");
    }
}

/// A round of a linear radius law along a box's straight edge, between its two faces at a
/// right angle: the ball of radius r(y) at the edge's fraction y lies r(y) in from each face.
struct LawPieces {
    from: Vec3,
    along: Vec3,
    inward: Vec3,
}

impl LawPieces {
    /// The edge from `from` through its middle `middle`, its start's rail ends `rails` at
    /// radius `r0`.
    fn new(from: [f64; 3], middle: [f64; 3], rails: [[f64; 3]; 2], r0: f64) -> Self {
        let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
        let from = v(from);
        let [a, b] = rails.map(|p| (v(p) - from) / r0);
        Self {
            from,
            along: (v(middle) - from) * 2.0,
            inward: a + b,
        }
    }

    /// How far `p` is off the envelope of the balls of radius `r(y)` centred at
    /// C(y) = from + y along + r(y) inward: h(y) = |p − C(y)|² − r(y)² is at least 0 for
    /// every y exactly where p is outside every ball, and 0 where it touches one. For a linear
    /// `r` it is a quadratic in y whose leading coefficient is |along|² + r′² > 0 (the edge is
    /// square to its faces' normals), so a ternary search over the edge and a length past each
    /// end finds its least value, which is 2 r (|p − C| − r) to first order: halved over r, a
    /// distance.
    fn off(&self, p: Vec3, r: impl Fn(f64) -> f64) -> f64 {
        let h = |y: f64| {
            let d = p - (self.from + self.along * y + self.inward * r(y));
            d.x * d.x + d.y * d.y + d.z * d.z - r(y) * r(y)
        };
        let (mut lo, mut hi) = (-1.0, 2.0);
        for _ in 0..200 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if h(a) < h(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let y = 0.5 * (lo + hi);
        (h(y) / (2.0 * r(y))).abs()
    }
}

/// Points of `pb`'s definition, on a 5 x 5 grid of its net.
fn definition_points(k: &Kernel, pb: &ProceduralBlend) -> Vec<Vec3> {
    let d = pb.net().natural_uv_domain();
    let (lo, hi) = (d.min(), d.max());
    let mut out = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (a, b) = (f64::from(i) / 4.0, f64::from(j) / 4.0);
            let uv = Point2::new(lo.x + a * (hi.x - lo.x), lo.y + b * (hi.y - lo.y));
            let st = pb.definition_parameters(uv);
            out.push(pb.evaluate_definition(k, st.x, st.y, 0, 0).unwrap()[0][0]);
        }
    }
    out
}

/// Select the cube's edge along x at y = z = 20, with its record Order.
fn variable_ordered_edge(b: &Body) -> (Order, usize) {
    let (record, order) = b.to_record().unwrap();
    let edge = record
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&v| {
                let p = record.vertices[usize::try_from(v).unwrap()].point;
                p[1] == 20.0 && p[2] == 20.0
            })
        })
        .unwrap();
    (order, edge)
}

#[test]
fn a_record_ordered_variable_round_keeps_its_selected_edge() {
    all_fixes();
    for permuted in [false, true] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        let (mut order, edge) = variable_ordered_edge(&b);
        let selected = order.edges[edge];
        if permuted {
            order.edges.rotate_left(5);
        }
        let edge = order.edges.iter().position(|&e| e == selected).unwrap();
        solidcore_ops::fillet_variable_ordered(
            &b,
            &order,
            edge,
            [-10.0, 20.0, 20.0],
            1,
            &[0.0, 0.0, 1.0, 1.0],
            &[2.0, 2.0],
        )
        .unwrap();
        let want = 8000.0 - (1.0 - PI / 4.0) * 4.0 * 20.0;
        assert!((b.volume(1e-9).unwrap() - want).abs() <= want * 1e-9);
        for point in [
            [0.0, 18.0, 20.0],
            [0.0, 20.0, 18.0],
            [20.0, 18.0, 20.0],
            [20.0, 20.0, 18.0],
        ] {
            vertex_at(&b, point);
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
    }
}

/// Reversing this law leaves its total volume the same, so its start's radius-4 rail
/// vertices and its ten faces, not volume alone, prove which end receives the law.
#[test]
fn a_record_ordered_variable_round_keeps_the_near_end_and_tie_rule() {
    all_fixes();
    for point in [[-10.0, 20.0, 20.0], [30.0, 20.0, 20.0], [10.0, 20.0, 20.0]] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        let (record, _) = b.to_record().unwrap();
        let (mut order, edge) = variable_ordered_edge(&b);
        let selected = order.edges[edge];
        let solidcore_record::brep::Curve::Line { start, .. } = record.edges[edge].curve else {
            panic!("the selected edge is a line");
        };
        let from_x = if point[0] < 0.0 {
            0.0
        } else if point[0] > 20.0 {
            20.0
        } else {
            start[0]
        };
        order.edges.reverse();
        let edge = order.edges.iter().position(|&e| e == selected).unwrap();
        solidcore_ops::fillet_variable_ordered(
            &b,
            &order,
            edge,
            point,
            1,
            &[0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
            &[4.0, 2.0, 4.0, 2.0],
        )
        .unwrap();
        assert_eq!(b.counts().unwrap()[0], 10, "start {point:?}");
        vertex_at(&b, [from_x, 16.0, 20.0]);
        vertex_at(&b, [from_x, 20.0, 16.0]);
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
    }
}

#[test]
fn an_ordered_variable_round_rejects_bad_selection_without_editing() {
    all_fixes();
    let b = make_box([0.0; 3], [20.0; 3]).unwrap();
    let (before, _) = b.to_record().unwrap();
    let (order, edge) = variable_ordered_edge(&b);
    let law = |order: &Order, edge: usize, start| {
        solidcore_ops::fillet_variable_ordered(
            &b,
            order,
            edge,
            start,
            1,
            &[0.0, 0.0, 1.0, 1.0],
            &[2.0, 2.0],
        )
    };
    assert_eq!(
        law(&order, usize::MAX, [0.0, 20.0, 20.0]),
        Err(Error::Argument)
    );
    let mut missing = order.clone();
    missing.edges[edge] = EdgeId::from_index(1_000_000);
    assert_eq!(law(&missing, edge, [0.0, 20.0, 20.0]), Err(Error::Argument));
    assert_eq!(
        law(&order, edge, [f64::NAN, 20.0, 20.0]),
        Err(Error::Argument)
    );
    assert_eq!(b.to_record().unwrap().0, before);
}

#[test]
fn arguments_are_checked_before_the_kernel() {
    all_fixes();
    let b = block();
    let e = top_front(&b);
    let round = |edges: &[usize], sizes: &[f64]| {
        blend(&b, Blend::Round, edges, sizes, &[], Corners::Filled)
    };
    assert_eq!(round(&[], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[e, e], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[999], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[e], &[0.0]), Err(Error::Argument));
    assert_eq!(round(&[e], &[f64::NAN]), Err(Error::Argument));
    assert_eq!(round(&[e], &[2e6]), Err(Error::Argument));
    assert_eq!(round(&[e], &[1.0, 2.0]), Err(Error::Argument));
    let corner = vertex_at(&b, [30.0, 0.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 1.0,
    }];
    assert!(matches!(
        blend(&b, Blend::Chamfer, &[e], &[1.0], &back, Corners::Filled),
        Err(Error::Unsupported(_))
    ));
    // a setback at a vertex the kernel does not set back (the end of one round), however
    // long: that corner is refused (#3145: the length is the kernel's, at a real corner)
    let long = [SetBack {
        vertex: corner,
        distance: 50.0,
    }];
    assert_eq!(
        blend(&b, Blend::Round, &[e], &[1.0], &long, Corners::Filled),
        Err(Error::CornerRefused { corner: 0 })
    );
    assert_eq!(
        chamfer_sides(&b, &[e], &[], 1.0, ChamferSecond::Distance(1.0)),
        Err(Error::Argument)
    );
    assert_eq!(
        chamfer_sides(&b, &[e], &[0], 1.0, ChamferSecond::Angle(PI)),
        Err(Error::Argument)
    );
    assert_eq!(
        edge_faces_along(&b, e, [f64::NAN, 0.0, 0.0]),
        Err(Error::Argument)
    );
    assert_eq!(
        edge_faces_along(&b, 999, [0.0, 0.0, 1.0]),
        Err(Error::Argument)
    );
    // nothing changed the body
    assert!(rel(volume(&b), ABC) < 1e-12);
}

/// A distance and an angle whose chamfer runs out past the other face: the refusal names the
/// angle and the bound below which one meets that face, pi less the wedge between the faces.
#[test]
fn a_chamfer_angle_past_the_other_face_is_refused_naming_its_bound() {
    all_fixes();
    let refused = |b: &Body, e: usize, angle: f64, largest: f64| {
        let first = edge_faces_along(b, e, [0.0, 0.0, -1.0]).unwrap()[0].0;
        let before = volume(b);
        match chamfer_sides(b, &[e], &[first], 2.0, ChamferSecond::Angle(angle)) {
            Err(Error::ChamferAngleTooLarge {
                edge,
                angle: asked,
                largest: bound,
            }) => {
                assert_eq!(edge, e);
                assert_eq!(asked, angle);
                assert!(rel(bound, largest) < 1e-12, "{bound} against {largest}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(volume(b), before, "a refusal leaves the body as it came in");
    };
    // a right angle between the faces: the top rim of a cylinder at 90.573 degrees
    let b = make_cylinder([0.0; 3], 10.0, 20.0).unwrap();
    refused(
        &b,
        edge_at(&b, [0.0, 10.0, 20.0]),
        1.580797063408824,
        PI / 2.0,
    );
    // and a straight edge, exactly at the bound as well as past it
    let b = block();
    let e = top_front(&b);
    refused(&b, e, PI / 2.0, PI / 2.0);
    refused(&b, e, 0.75 * PI, PI / 2.0);
    // 45 degrees between a frustum's base and its side: room for 135
    let b = make_cone([0.0; 3], 10.0, 5.0, 5.0).unwrap();
    refused(&b, edge_at(&b, [0.0, 10.0, 0.0]), 2.5, 0.75 * PI);
}

/// The index of the edge of `b` nearest `p` (a case's pick lies off its edge).
fn nearest_edge(b: &Body, p: [f64; 3]) -> usize {
    let (k, brep) = kernel_copy(b);
    let q = Vec3::new(p[0], p[1], p[2]);
    let distance = |&e: &EdgeId| {
        edge_closest_point(&k, &brep, e, q)
            .unwrap()
            .closest
            .distance
    };
    let list = brep.edge_list();
    let all = list.iter().map(distance);
    all.enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0
}

/// A tangent chain chamfered 1 and 2 is one chamfer: the tilted cylinder's elliptical rim, in
/// two arcs whose middles have different wedges (the kernel case
/// `2893-chamfer-two-distances-elliptical-rim`), measured at the narrowest wedge at which no
/// station folds: the wedge opens to 90 degrees plus the tilt at the rim's far tip, inside an
/// arc, and the smallest setback there keeps 1000 edge tolerances. Its one pair of offsets
/// puts the ball's centre on the cylinder of radius 5 less the wall's offset, at the
/// top's offset under the plane z = 10, so the top's rail is that cylinder's ellipse there,
/// lifted to the plane: semi-axes `R` along y and `R sqrt(5) / 2` along x, centred at
/// x = 10 less half the top's offset (the axis runs 1 along x to 2 along z).
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_tangent_chain_is_one_chamfer_measured_where_no_station_folds() {
    const SQRT_5: f64 = 2.236_067_977_499_79;
    all_fixes();
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fillets/",
        "2893-chamfer-two-distances-elliptical-rim.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b12"].clone()).unwrap();
    let (b, _) = Body::from_record(&record, true).unwrap();
    let up = [0.0, 0.0, 1.0];
    let rim = [
        nearest_edge(
            &b,
            [12.397279543126551, 4.505296518190091, 9.999999999999998],
        ),
        nearest_edge(
            &b,
            [7.602194600606031, -4.505901858302108, 9.999999999999998],
        ),
    ];
    assert_ne!(rim[0], rim[1]);
    // each arc's faces at its middle: one is the top, whose normal is up
    let mut top = 0;
    for e in rim {
        let [a, c] = edge_faces_along(&b, e, up).unwrap();
        let t = if a.1 > c.1 { a } else { c };
        assert!(rel(t.1, 1.0) < 1e-12);
        top = t.0;
    }
    let (_k, before) = kernel_copy(&b);
    let tolerance = rim
        .iter()
        .map(|&e| edge::tolerance(&before, before.edge_list()[e]).unwrap())
        .fold(0.0, f64::max);
    // At the widest wedge cos w = -1 / sqrt 5, sin w = 2 / sqrt 5. The kernel encloses it from
    // above, its normals' cosine within a gap of `tolerance / (1 + 2)`, so the cos w it uses lies
    // in [cos w - gap, cos w]; `unfolded_wedge`'s bound at each end of that range gives the rail's
    // ellipse at each end, and the built one lies between.
    let gap = tolerance / 3.0;
    let ellipse = |cos_w: f64| {
        let sin_w = (1.0 - cos_w * cos_w).sqrt();
        let c = (1.0 + 2.0 * cos_w - 1000.0 * tolerance * sin_w) / (2.0 + cos_w);
        let s = c.acos().sin();
        let (r_top, r_wall) = ((2.0 - c) / s, (1.0 - 2.0 * c) / s);
        let radius = 5.0 - r_wall;
        let half_x = radius * SQRT_5 / 2.0;
        let middle = 10.0 - r_top / 2.0;
        [radius, middle + half_x, middle - half_x]
    };
    let ends = [ellipse(-1.0 / SQRT_5), ellipse(-1.0 / SQRT_5 - gap)];
    chamfer_sides(&b, &rim, &[top, top], 1.0, ChamferSecond::Distance(2.0)).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report:?}");
    let (k, made) = kernel_copy(&b);
    let bx = made
        .face_list()
        .iter()
        .find_map(|&f| {
            let bx = face_bounding_box(&k, &made, f, true).ok()?;
            ((bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9).then_some(bx)
        })
        .unwrap();
    // each measure between its values at the two ends, to 1e-8 of it
    let between = |got: f64, i: usize| {
        let (a, b) = (ends[0][i], ends[1][i]);
        got >= a.min(b) - 1e-8 * a.abs() && got <= a.max(b) + 1e-8 * a.abs()
    };
    assert!(between(bx.max().y, 0), "{} {ends:?}", bx.max().y);
    assert!(between(-bx.min().y, 0), "{} {ends:?}", bx.min().y);
    assert!(between(bx.max().x, 1), "{} {ends:?}", bx.max().x);
    assert!(between(bx.min().x, 2), "{} {ends:?}", bx.min().x);
}

#[test]
fn a_round_larger_than_its_faces_is_refused_with_the_size_that_fits() {
    all_fixes();
    // a 2 thick wall, both its top edges rounded 1.5: the two rounds' bands overlap on the top
    let b = make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [15.0, 2.0, 10.0]),
    ];
    let r = blend(&b, Blend::Round, &edges, &[1.5], &[], Corners::Filled);
    match r {
        Err(Error::TooLarge { size, kind, at, .. }) => {
            assert_eq!(kind, FitKind::Radius);
            assert!(size > 0.0 && size <= 1.0 + 1e-9, "{size}");
            let at = at.unwrap();
            assert!((at[2] - 10.0).abs() < 1e-9, "{at:?}");
        }
        Err(Error::NoRoom) => {}
        other => panic!("{other:?}"),
    }
    // refused: the body as it came in
    assert!(rel(volume(&b), 600.0) < 1e-12);
}

/// A body the kernel sweeps along +z, `height` high, from a closed profile in z = 0.
fn swept(profile: &[solidcore::curve::curve::Curve], height: f64) -> Body {
    let mut k = Kernel::default();
    let refs: Vec<Option<&solidcore::curve::curve::Curve>> = profile.iter().map(Some).collect();
    let b = solidcore::sweep::api_brep::create_linear_sweep(
        &mut k,
        &refs,
        Vec3::new(0.0, 0.0, 1.0),
        height,
        true,
    )
    .unwrap();
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &loopuse::notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    Body::from_smb(
        write_brep_to_file(&k, Some(&b), &env, false).unwrap(),
        false,
    )
    .unwrap()
}

fn segment(a: (f64, f64), b: (f64, f64)) -> solidcore::curve::curve::Curve {
    solidcore::curve::curve::Curve::from(
        solidcore::primitive::api_curves::create_line_segment(
            Vec3::new(a.0, a.1, 0.0),
            Vec3::new(b.0, b.1, 0.0),
        )
        .unwrap(),
    )
}

/// The arc about `c` of radius `r` from `from` to `to` degrees.
fn arc_deg(c: (f64, f64), r: f64, from: f64, to: f64) -> solidcore::curve::curve::Curve {
    solidcore::curve::curve::Curve::from(
        solidcore::primitive::api_curves::create_arc(Vec3::new(c.0, c.1, 0.0), r, from, to)
            .unwrap(),
    )
}

/// Half a degree, in radians.
const HALF_DEGREE: f64 = 0.008_726_646_259_971_648;

/// A slot 2 high: two straights 10 long, 4 apart, joined by half circles of radius 2.
fn slot() -> Body {
    swept(
        &[
            segment((0.0, 0.0), (10.0, 0.0)),
            arc_deg((10.0, 2.0), 2.0, -90.0, 90.0),
            segment((10.0, 4.0), (0.0, 4.0)),
            arc_deg((0.0, 2.0), 2.0, 90.0, 270.0),
        ],
        2.0,
    )
}

/// The slot's top outline is one tangent chain: from its front straight, all four top edges;
/// a box edge runs on into nothing. Indices are checked before the kernel.
#[test]
fn the_tangent_chain_runs_round_the_slot() {
    all_fixes();
    let b = slot();
    let front = edge_at(&b, [5.0, 0.0, 2.0]);
    let chain = tangent_chain(&b, None, &[front], &[], HALF_DEGREE).unwrap();
    assert!(chain.undecided.is_empty(), "{chain:?}");
    let chain = chain.members;
    assert_eq!(chain.len(), 4, "{chain:?}");
    assert_eq!(chain[0], front);
    for p in [[5.0, 4.0, 2.0], [12.0, 2.0, 2.0], [-2.0, 2.0, 2.0]] {
        assert!(chain.contains(&edge_at(&b, p)), "{p:?}");
    }
    let bx = block();
    let e = top_front(&bx);
    let chain = |edges: &[usize], angle: f64| tangent_chain(&bx, None, edges, &[], angle);
    assert_eq!(chain(&[e], HALF_DEGREE).unwrap().members, vec![e]);
    assert_eq!(chain(&[99], 0.01), Err(Error::Argument));
    assert_eq!(chain(&[e], 0.0), Err(Error::Argument));
    assert_eq!(chain(&[], 0.01), Err(Error::Argument));
}

/// The slot's seam, where its straight side runs into its round end, has no blend: its faces
/// meet tangentially all along it. Refused with its index, the body as it came in.
#[test]
fn an_edge_with_no_blend_is_named() {
    all_fixes();
    let b = slot();
    let seam = edge_at(&b, [10.0, 0.0, 1.0]);
    let before = volume(&b);
    let r = blend(&b, Blend::Round, &[seam], &[0.5], &[], Corners::Filled);
    assert_eq!(
        r,
        Err(Error::EdgeNotBlendable {
            edge: seam,
            reason: Unblendable::Smooth,
        })
    );
    assert!(rel(volume(&b), before) < 1e-12);
}

/// A crease whose faces turn by atan(0.0875) (5 degrees) at x = 10: a round of 1e-4 lies within
/// the edge's tolerance of it and is refused, naming the smallest radius that rounds it; a
/// round of 0.5 builds.
#[test]
fn a_round_too_small_to_change_the_part_is_refused() {
    all_fixes();
    let crease = || {
        swept(
            &[
                segment((0.0, 0.0), (10.0, 0.0)),
                segment((10.0, 0.0), (20.0, 0.875)),
                segment((20.0, 0.875), (20.0, 10.0)),
                segment((20.0, 10.0), (0.0, 10.0)),
                segment((0.0, 10.0), (0.0, 0.0)),
            ],
            5.0,
        )
    };
    let b = crease();
    let e = edge_at(&b, [10.0, 0.0, 2.5]);
    let before = volume(&b);
    match blend(&b, Blend::Round, &[e], &[1e-4], &[], Corners::Filled) {
        Err(Error::RoundTooSmall {
            edge,
            turn,
            tolerance,
            smallest,
        }) => {
            assert_eq!(edge, e);
            // atan(0.0875), to the kernel's evaluation
            assert!((turn - 0.087_277_712_949_461_45).abs() < 1e-12, "{turn}");
            assert!(tolerance > 0.0 && smallest > 1e-4, "{tolerance} {smallest}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), before) < 1e-12);
    let b = crease();
    blend(&b, Blend::Round, &[e], &[0.5], &[], Corners::Filled).unwrap();
    assert!(volume(&b) < before);
}

/// solidcore#2250's box: 40 x 30 x 20, its top back edge chamfered 5 (V0 23500); A, the
/// chamfer's top edge (a 135 degree wedge), and B, the bottom front edge (90 degrees), rounded
/// curvature-continuously r 2 in one call, in either order. Each round takes off its own ball's
/// area, r^2 (cot(a/2) - (pi - a)/2) per unit length (0.0860579227 for A, 0.8584073464 for
/// B), along 40: the kernel resolves the equal-area thumbweight from each station's own wedge.
#[test]
fn curvature_rounds_over_mixed_wedges_each_take_their_own_balls_area() {
    all_fixes();
    let per_length = 0.086_057_922_7 + 0.858_407_346_4;
    let run = |a_first: bool| -> f64 {
        let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
        let back = edge_at(&b, [20.0, 30.0, 20.0]);
        blend(&b, Blend::Chamfer, &[back], &[5.0], &[], Corners::Filled).unwrap();
        assert!(rel(volume(&b), 23500.0) < 1e-9);
        let a = edge_at(&b, [20.0, 25.0, 20.0]);
        let bottom = edge_at(&b, [20.0, 0.0, 0.0]);
        let edges = if a_first { [a, bottom] } else { [bottom, a] };
        blend(&b, Blend::Curvature, &edges, &[2.0], &[], Corners::Filled).unwrap();
        23500.0 - volume(&b)
    };
    let ab = run(true);
    let ba = run(false);
    assert!((ab - 40.0 * per_length).abs() < 1e-5, "[A, B] took {ab}");
    assert!((ba - 40.0 * per_length).abs() < 1e-5, "[B, A] took {ba}");
    assert!((ab - ba).abs() < 1e-7, "{ab} {ba}");
}

/// #3145: a chamfer too large for its faces is refused with the distance that builds, which the
/// kernel reads through the chamfer's definition (its offsets linear in its sizes at the edge's
/// wedge), and that distance builds. A 2 thick wall, both its top edges chamfered 1.5 at
/// equal distances: square edges, so each chamfer's setback is its distance, and the two bands
/// on the top meet when the setbacks are 1 each: the fit is 1. Chamfered at it, each edge
/// loses the prism of the right triangle of side 1 along the wall's 30: `2 * 30 / 2` removed.
#[test]
fn a_chamfer_too_large_is_refused_with_the_distance_that_builds() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [15.0, 2.0, 10.0]),
    ];
    let size = match blend(&b, Blend::Chamfer, &edges, &[1.5], &[], Corners::Filled) {
        Err(Error::TooLarge { size, kind, .. }) => {
            assert_eq!(kind, FitKind::Distance);
            size
        }
        other => panic!("{other:?}"),
    };
    // the kernel's fit read through the chamfer's offsets and back: lines, so no quadrature,
    // only the fit's and the conversions' rounding (measured 1.1e-13)
    assert!((size - 1.0).abs() <= 1e-12, "{size}");
    let before = volume(&b);
    blend(&b, Blend::Chamfer, &edges, &[size], &[], Corners::Filled).unwrap();
    let removed = before - volume(&b);
    let want = 30.0 * size * size;
    assert!(rel(removed, want) <= 1e-9, "{removed} vs {want}");
}

/// #3145: a setback longer than an edge at a real set-back corner is the kernel's refusal before
/// the trace, naming the largest that fits: the block's corner at (30, 0, 10) with its three
/// edges rounded 1 and set back 50, its shortest edge 10 long. A setback of 10 would leave that
/// edge's round no length (an open bound), so the size named is 10 less a thousand of the
/// edge's tolerances; set back that, the block builds, and just above it is refused. Without
/// the check the trace refused first, as a round too large (radius 19.99, the corners taking a
/// whole edge).
#[test]
fn a_setback_too_long_at_a_corner_is_refused_by_the_kernel() {
    all_fixes();
    let b = block();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [30.0, 10.0, 10.0]),
        edge_at(&b, [30.0, 0.0, 5.0]),
    ];
    let corner = vertex_at(&b, [30.0, 0.0, 10.0]);
    let set_back = |distance: f64| {
        [SetBack {
            vertex: corner,
            distance,
        }]
    };
    let tol = {
        let (_k, kb) = kernel_copy(&b);
        edge::tolerance(&kb, kb.edge_list()[edges[2]]).unwrap()
    };
    let named = match blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(50.0),
        Corners::Filled,
    ) {
        Err(Error::SetbackTooLong { vertex, largest }) => {
            assert_eq!(vertex, corner);
            // the least of the three edges' lengths (lines: exact), the far ends set back none,
            // less the fit margin
            let want = 10.0 - 1000.0 * tol;
            assert!((largest - want).abs() <= 1e-12, "{largest} against {want}");
            largest
        }
        other => panic!("{other:?}"),
    };
    // just above the named size: refused by the same test, the block unchanged
    match blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(named + 1e-9),
        Corners::Filled,
    ) {
        Err(Error::SetbackTooLong { vertex, .. }) => assert_eq!(vertex, corner),
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), 6000.0) < 1e-12);
    // at it: the block builds (the rounds take volume)
    blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(named),
        Corners::Filled,
    )
    .unwrap();
    assert!(volume(&b) < 6000.0);
}

/// Cancel stops a long fillet promptly (solidcore#3247): a 2 mm round along one helical edge of
/// a 20-turn square coil, with its tangent chain (the whole edge), runs for minutes. Measured in
/// the token's polls, not in time: a token cancelled from its `m`-th poll
/// (`CancelToken::after_polls`) stops the fillet there. It returns `Cancelled`, which also shows
/// the fillet was still running at poll `m`, and the token is polled exactly once after the poll
/// that cancelled it: by the session as the call returns, asking whether it was cancelled
/// (`session::run_with`, or `with_body` deciding whether to keep its edit). Any further poll
/// would be work done after the cancel. `m` is a quarter of the 15,473 polls the fillet made in
/// its first second, measured on SolidCraft 9a342278 (cancel-timing round 1). Should the fillet
/// get fast enough to finish before `m`, this fails with `Ok`: then pick a longer case.
#[test]
fn a_cancelled_long_fillet_stops_promptly() {
    use solidcore_ops::cancel::{within, CancelToken};
    use solidcore_ops::sweep::{screw_sweep, Curve};
    all_fixes();
    let corners = [
        [19.0, 0.0, 0.0],
        [32.0, 0.0, 0.0],
        [32.0, 0.0, 13.0],
        [19.0, 0.0, 13.0],
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let coil = screw_sweep(
        &square,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        15.24,
        20.0,
        None,
        15.24e-4,
    )
    .unwrap();
    // the inner lower helix half a turn in, whichever way the sweep turns
    let (k, brep) = kernel_copy(&coil);
    let q = Vec3::new(-19.0, 0.0, 15.24 / 2.0);
    let edge = (0..brep.edge_list().len())
        .min_by(|&a, &b| {
            let d = |i: usize| {
                edge_closest_point(&k, &brep, brep.edge_list()[i], q)
                    .unwrap()
                    .closest
                    .distance
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let chain = tangent_chain(&coil, None, &[edge], &[], 0.1)
        .unwrap()
        .members;
    let m: u64 = 15_473 / 4;
    let token = CancelToken::after_polls(m);
    let r = within(&token, || {
        blend(&coil, Blend::Round, &chain, &[2.0], &[], Corners::Filled)
    });
    eprintln!("the fillet cancelled at {m}, polled {:?}", token.polls());
    assert_eq!(r.map(|_| ()), Err(Error::Cancelled));
    assert_eq!(token.polls(), Some(m + 1));
}

/// A failure the kernel types as not supported, after the session noted no room with a fit
/// (or an unsupported vertex), is its own cause: [`solidcore_ops::blend::session_refusal`]
/// gives it back rather than sizing it from the earlier note (SolidCraft's own mapping made it
/// `TooLarge` before it took ops'). The note is real: a 20 cube's three corner edges rounded
/// 20, refused naming 19.99 (the case of SolidCraft's `a_cliff_overflow_the_fork_refuses_has_no_room`).
#[test]
fn a_kernel_unsupported_after_a_no_room_note_is_its_own_cause() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::SolidError;
    use solidcore_ops::blend::{session_refusal, Refusal, SessionCall};
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let at = [
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 10.0),
    ];
    let edges: Vec<EdgeId> = at
        .iter()
        .map(|&q| {
            b.edges()
                .ids()
                .min_by(|&x, &y| {
                    let d = |e| edge_closest_point(&k, &b, e, q).unwrap().closest.distance;
                    d(x).total_cmp(&d(y))
                })
                .unwrap()
        })
        .collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    let circular = [FilletSurfaceGeneratorType::Circular];
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[20.0; 3], &circular, 1, 1.0)
        .unwrap();
    s.create_corners(&k, None).unwrap();
    let refused = s.do_filleting(&mut k, None).unwrap_err();
    // the note: no room, with the fit
    let sized = session_refusal(&s, refused, SessionCall::default()).unwrap();
    let Refusal::TooLarge { size, .. } = sized.on(&k, s.target()).unwrap() else {
        panic!("not sized")
    };
    assert!((size - 19.99).abs() < 1e-9, "{size}");
    let unsupported = SolidError::Unsupported {
        operation: "test",
        reason: "a construction the kernel does not build",
    };
    for corner in [false, true] {
        let call = SessionCall {
            corner,
            ..SessionCall::default()
        };
        assert_eq!(
            session_refusal(&s, unsupported.clone(), call).map(|_| ()),
            Err(unsupported.clone()),
            "corner {corner}"
        );
    }
}
