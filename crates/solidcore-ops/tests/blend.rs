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

use solidcore_ops::{
    blend, blend_ordered, chamfer_sides, edge_faces_along, fillet_variable, make_box, make_cone,
    make_cylinder, tangent_chain, Blend, Body, ChamferSecond, CheckLevel, Corners, Error, FitKind,
    Order, SetBack, Turn, Unblendable,
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
            turn,
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
            Turn::Swept,
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
        Turn::Mitre,
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
    // a setback longer than its edges leaves no room
    let long = [SetBack {
        vertex: corner,
        distance: 50.0,
    }];
    assert_eq!(
        blend(&b, Blend::Round, &[e], &[1.0], &long, Corners::Filled),
        Err(Error::NoRoom)
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
    let [a, c] = edge_faces_along(&b, rim[0], up).unwrap();
    let top = if a.1 > c.1 { a.0 } else { c.0 };
    let (_k, before) = kernel_copy(&b);
    let tolerance = rim
        .iter()
        .map(|&e| edge::tolerance(&before, before.edge_list()[e]).unwrap())
        .fold(0.0, f64::max);
    // at the widest wedge cos w = -1 / sqrt 5, sin w = 2 / sqrt 5; `unfolded_wedge`'s bound
    let (cos_w, sin_w) = (-1.0 / SQRT_5, 2.0 / SQRT_5);
    let c = (1.0 + 2.0 * cos_w - 1000.0 * tolerance * sin_w) / (2.0 + cos_w);
    let s = c.acos().sin();
    let (r_top, r_wall) = ((2.0 - c) / s, (1.0 - 2.0 * c) / s);
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
    let radius = 5.0 - r_wall;
    let half_x = radius * SQRT_5 / 2.0;
    assert!(rel(bx.max().y, radius) < 1e-7, "{}", bx.max().y);
    assert!(rel(bx.min().y, -radius) < 1e-7, "{}", bx.min().y);
    let middle = 10.0 - r_top / 2.0;
    assert!(rel(bx.max().x, middle + half_x) < 1e-7, "{}", bx.max().x);
    assert!(rel(bx.min().x, middle - half_x) < 1e-7, "{}", bx.min().x);
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
fn the_tangent_chain_is_the_kernels() {
    all_fixes();
    let b = slot();
    let front = edge_at(&b, [5.0, 0.0, 2.0]);
    let chain = tangent_chain(&b, &[front], HALF_DEGREE).unwrap();
    assert_eq!(chain.len(), 4, "{chain:?}");
    assert_eq!(chain[0], front);
    for p in [[5.0, 4.0, 2.0], [12.0, 2.0, 2.0], [-2.0, 2.0, 2.0]] {
        assert!(chain.contains(&edge_at(&b, p)), "{p:?}");
    }
    let bx = block();
    let e = top_front(&bx);
    assert_eq!(tangent_chain(&bx, &[e], HALF_DEGREE).unwrap(), vec![e]);
    assert_eq!(tangent_chain(&bx, &[99], 0.01), Err(Error::Argument));
    assert_eq!(tangent_chain(&bx, &[e], 0.0), Err(Error::Argument));
    assert_eq!(tangent_chain(&bx, &[], 0.01), Err(Error::Argument));
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
