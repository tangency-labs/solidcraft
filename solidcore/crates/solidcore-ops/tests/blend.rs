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

use solidcore_ops::sweep::{extrude, Curve, DraftCorner, SweepFrame};
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

/// The plate of [`blends_turn_round_a_shallow_sharp_corner`]: 2 thick, its top outline 50
/// along x and then 40 turning `kink` degrees (towards the plate for `convex`, away from it
/// else), with the record indices of those two top edges.
fn shallow_corner_plate(kink: f64, convex: bool) -> (Body, [usize; 2]) {
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
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [0.5, 1.5, 3.0, 6.0] {
        for convex in [true, false] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
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

/// Asserts that the faces of [`shallow_corner_plate`]`(kink, convex)` rounded r 1 that are not
/// planes are two cylinders, one per round, read from their definitions: radius 1, semi-angle
/// 0, the axis parallel to the round's edge and its origin on the line through the points 1 in
/// from both of the edge's faces (the edge moved 1 into the plate and 1 down), each within
/// 1e-12 (rounding of coordinates up to 90 and unit vectors). One round's surface extended over
/// both, as the cleanup used to make it, stands 1.7e-7 rad off the other axis at 1e-5 degrees.
fn rounds_on_their_cylinders(b: &Body, kink: f64, convex: bool) {
    use solidcore::topology::face::p2 as face;
    let t = if convex { -kink } else { kink }.to_radians();
    let (u, n) = ([t.cos(), t.sin()], [t.sin(), -t.cos()]);
    // (a point, the direction) of each round's axis
    let axes = [
        (Vec3::new(0.0, -1.0, 1.0), Vec3::new(1.0, 0.0, 0.0)),
        (
            Vec3::new(50.0 + n[0], n[1], 1.0),
            Vec3::new(u[0], u[1], 0.0),
        ),
    ];
    let (_k, brep) = kernel_copy(b);
    let mut on = Vec::new();
    for &f in brep.face_list() {
        let s = face::surface(&brep, f).unwrap().unwrap();
        let cone = match s.kind() {
            SurfaceKind::Plane(_) => continue,
            SurfaceKind::Cone(c) => c.canonical(),
            other => panic!("kink {kink} (convex {convex}): a round on {other:?}"),
        };
        assert!(
            (cone.radius - 1.0).abs() <= 1e-12 && cone.semi_angle_deg == 0.0,
            "kink {kink} (convex {convex}): radius {} semi-angle {}",
            cone.radius,
            cone.semi_angle_deg
        );
        let (origin, axis) = (cone.origin.origin(), cone.origin.z_axis());
        let off: Vec<(f64, f64)> = axes
            .iter()
            .map(|(a, d)| (axis.cross(*d).length(), (origin - *a).cross(*d).length()))
            .collect();
        let which = (0..2)
            .find(|&i| off[i].0 <= 1e-12 && off[i].1 <= 1e-12)
            .unwrap_or_else(|| {
                panic!("kink {kink} (convex {convex}): a round {off:?} off both axes")
            });
        on.push(which);
    }
    on.sort_unstable();
    assert_eq!(
        on,
        [0, 1],
        "kink {kink} (convex {convex}): rounds on cylinders {on:?}"
    );
}

/// #15418: the plate of [`blends_turn_round_a_shallow_sharp_corner`] with corners a hundred
/// thousandth to a few ten thousandths of a degree off straight. The two top edges rounded r 1
/// or chamfered 1 build and pass every check. Each round stays its own face on its own
/// closed-form cylinder: radius 1 about the line 1 in from the edge's two faces. The volume
/// taken off is the mitred corner's closed form (the section's area A along the 90 of edge,
/// the corner's first moment M times tan(kink / 2) twice, off at the convex corner and on at
/// the concave one) within the 1e-6 of the sibling test: these corners join on one section,
/// whose volume differs from the mitre's by a multiple of the kink. Rounds at a concave corner
/// 3e-5 to 2e-4 degrees off straight used to come out as one cylinder face, the boolean's
/// cleanup merging the second round into the first's extended surface: its far end stood the
/// first edge's length times the kink off it and failed the check (`SmVertex`'s gap test).
#[test]
fn blends_turn_round_a_corner_a_ten_thousandth_of_a_degree_off_straight() {
    all_fixes();
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [1e-5, 5e-5, 1e-4, 2e-4, 3e-4] {
        for convex in [false, true] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                assert!(
                    b.check(CheckLevel::All).unwrap().is_valid(),
                    "{kind:?} at a {kink} degree corner (convex {convex})"
                );
                if kind == Blend::Round {
                    rounds_on_their_cylinders(&b, kink, convex);
                }
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

/// #16627: the plate of [`blends_turn_round_a_shallow_sharp_corner`] with corners a thousandth
/// to a hundredth of a degree off straight, both top edges rounded r 1 or chamfered 1. Each
/// result passes every check, each round lies on its own closed-form cylinder, and the volume
/// taken off is the mitred corner's closed form within the sibling tests' 1e-6. Where the two
/// blends' sections at the corner stand 1 x kink apart, a Join puts one vertex there for both,
/// which the check holds within the B-rep's intersection tolerance (2e-5): concave corners
/// 1.5e-3 and 2e-3 degrees off straight (2.6e-5 and 3.5e-5 apart) used to be joined and failed
/// `SmVertex`'s gap test; now they are mitred. The bound is 2e-5 rad, 1.146e-3 degrees:
/// 1.14e-3 still joins, 1.15e-3 is mitred (`wr697_end_and_join.rs` asserts which). Asymmetric
/// chamfers (setbacks 1 and 0.5 either way round) stand at most their larger setback times the
/// kink apart and pass the check on both sides of the bound too.
#[test]
fn blends_join_at_a_near_straight_corner_only_within_the_vertex_check() {
    all_fixes();
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [1e-3, 1.14e-3, 1.15e-3, 1.5e-3, 2e-3] {
        for convex in [false, true] {
            for (top, side) in [(1.0, 0.5), (0.5, 1.0)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let along: Vec<usize> = edges
                    .iter()
                    .map(|&e| {
                        let faces = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
                        if faces[0].1 > faces[1].1 {
                            faces[0].0
                        } else {
                            faces[1].0
                        }
                    })
                    .collect();
                chamfer_sides(&b, &edges, &along, top, ChamferSecond::Distance(side))
                    .unwrap_or_else(|e| {
                        panic!("chamfer {top} x {side} at {kink} degrees (convex {convex}): {e:?}")
                    });
                let report = b.check(CheckLevel::All).unwrap();
                assert!(
                    report.is_valid(),
                    "chamfer {top} x {side} at {kink} degrees (convex {convex}): {report}"
                );
            }
        }
    }
    for kink in [1e-3, 1.14e-3, 1.15e-3, 1.5e-3, 2e-3, 3e-3, 5e-3, 1e-2] {
        for convex in [false, true] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                let report = b.check(CheckLevel::All).unwrap();
                assert!(
                    report.is_valid(),
                    "{kind:?} at a {kink} degree corner (convex {convex}): {report}"
                );
                if kind == Blend::Round {
                    rounds_on_their_cylinders(&b, kink, convex);
                }
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

/// A sketch outline with a near-tangent corner, the whole top rim rounded: a four-sided plate 5
/// thick, its top outline 40 along x, then 40 on at 3.8 degrees off it (two lines meeting at
/// 176.2 degrees), then back to (0, -30) and up to the start. All four top edges rounded r 1.5
/// and r 3, one closed chain through the 3.8 degree corner and the three sharp ones. Every corner
/// is convex and mitred: as in [`blends_turn_round_a_shallow_sharp_corner`], the rim loses
/// A = (1 - pi/4) r^2 along its perimeter P, less 2 M tan(theta / 2) at each corner turning
/// theta, M = (5/6 - pi/4) r^3. The rounds become 4 faces on the plate's 6. Before the shallow
/// corner was mitred, the two rounds meeting at it were given one shared section and the blend
/// failed with status 1001.
#[test]
fn a_rim_round_runs_through_a_near_tangent_corner() {
    all_fixes();
    let t = (-3.8_f64).to_radians();
    let p = [
        [0.0, 0.0],
        [40.0, 0.0],
        [40.0 + 40.0 * t.cos(), 40.0 * t.sin()],
        [0.0, -30.0],
    ];
    let side = |i: usize| {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        [b[0] - a[0], b[1] - a[1]]
    };
    let len = |v: [f64; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let perimeter: f64 = (0..4).map(|i| len(side(i))).sum();
    // tan of half the turn at each corner, from the sides into and out of it
    let half_turns: f64 = (0..4)
        .map(|i| {
            let (a, b) = (side((i + 3) % 4), side(i));
            let cos = (a[0] * b[0] + a[1] * b[1]) / (len(a) * len(b));
            (cos.acos() / 2.0).tan()
        })
        .sum();
    let area = 0.5
        * (0..4)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 4]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            .abs();
    for r in [1.5, 3.0] {
        let profile: Vec<Curve> = (0..4)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 4]);
                Curve::Line {
                    start: [a[0], a[1], 0.0],
                    end: [b[0], b[1], 0.0],
                }
            })
            .collect();
        let b = extrude(
            &profile,
            [0.0, 0.0, 1.0],
            5.0,
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap();
        let before = b.volume(1e-10).unwrap();
        assert!(rel(before, 5.0 * area) < 1e-12, "r {r}: plate {before}");
        let edges: Vec<usize> = (0..4)
            .map(|i| {
                let (a, c) = (p[i], p[(i + 1) % 4]);
                edge_at(&b, [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0, 5.0])
            })
            .collect();
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        assert_eq!(b.counts().unwrap()[0], 10, "r {r}");
        let want = (1.0 - PI / 4.0) * r * r * perimeter
            - 2.0 * (5.0 / 6.0 - PI / 4.0) * r.powi(3) * half_turns;
        let off = before - b.volume(1e-10).unwrap();
        assert!(rel(off, want) < 1e-9, "r {r}: {off} off, want {want}");
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

/// tangency-labs/solidcraft#16897: an 80 x 60 x 10.5 plate with a 30 x 3 x 65 tab on it
/// (x -15..15, y -1.5..1.5), the tab's four base edges rounded. Past r 1.5 the rounds of the
/// tab's 3 long ends run on past their end face's own extent to meet the long sides' rounds in
/// their mitres (0.41 built them only up to r 1.5).
///
/// The closed form: the plate and tab hold 80 * 60 * 10.5 + 30 * 3 * 65 = 56250. Each base
/// edge gains the spandrel A = (1 - pi/4) r^2 (the square of side r less its quarter disc)
/// along its length, 2 * 30 + 2 * 3 = 66 in all. At each of the four upright tab edges the two
/// mitred rounds add their parts past both walls, 2 M with M = (5/6 - pi/4) r^3, as in
/// [`rounds_turn_round_a_boss_edge_mitred_or_swept`]. The corners at either end of a 3 long
/// edge lie on opposite sides of it (y > 1.5 and y < -1.5), so they never overlap however
/// large r is: V = 56250 + 66 A + 8 M.
#[test]
fn rounds_turn_round_a_thin_tab_wider_than_its_end() {
    all_fixes();
    let base = [[-15.0, -1.5], [15.0, -1.5], [15.0, 1.5], [-15.0, 1.5]];
    for r in [1.5, 2.0, 3.0] {
        let b = built(|k| {
            let plate = create_box(k, Vec3::new(-40.0, -30.0, 0.0), 80.0, 60.0, 10.5)?;
            let tab = create_box(k, Vec3::new(-15.0, -1.5, 10.5), 30.0, 3.0, 65.0)?;
            boolean_union(k, plate, tab)
        });
        let (order, edges) = loop_edges(&b, base, 10.5);
        blend_ordered(
            &b,
            &order,
            Blend::Round,
            &edges,
            &[r],
            &[],
            Corners::Filled,
            BlendOptions::default(),
        )
        .unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let (a, m) = ((1.0 - PI / 4.0) * r * r, (5.0 / 6.0 - PI / 4.0) * r * r * r);
        let v = b.volume(1e-10).unwrap();
        assert!(rel(v, 56250.0 + 66.0 * a + 8.0 * m) < 1e-9, "r {r}: {v}");
    }
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

/// #17511: a round of 8 on a quarter circle of radius 10 that meets a straight side 16.7
/// degrees off tangent. On the top, the round of the arc touches the circle of radius 2 about
/// the arc's centre and the round of the side touches the line 8 in from it; they cross twice,
/// 2.4 apart, and the corner is where the arc's own round meets the side's: on the arc's side of
/// its end. The arc is shorter than twice the radius, so a guess measured along the edges lies
/// near the arc's far end, and the solve found the other crossing, a ball past the arc's end,
/// whose mitre with the side's round did not reach it. The crossing nearest the vertex builds.
#[test]
fn a_round_on_an_arc_shorter_than_its_diameter_meets_the_next_side_at_its_own_crossing() {
    all_fixes();
    let b = swept(
        &[
            segment((0.0, 0.0), (30.0, 0.0)),
            segment((30.0, 0.0), (30.0, 20.0)),
            arc_deg((20.0, 20.0), 10.0, 0.0, 90.0),
            segment((20.0, 30.0), (0.0, 24.0)),
            segment((0.0, 24.0), (0.0, 0.0)),
        ],
        10.0,
    );
    let half = 0.5_f64.sqrt() * 10.0;
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [30.0, 10.0, 10.0]),
        edge_at(&b, [20.0 + half, 20.0 + half, 10.0]),
        edge_at(&b, [10.0, 27.0, 10.0]),
        edge_at(&b, [0.0, 12.0, 10.0]),
    ];
    let before = volume(&b);
    blend(&b, Blend::Round, &edges, &[8.0], &[], Corners::Filled).unwrap();
    assert!(volume(&b) < before);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    // the side's unit direction from the arc's end and its normal into the part
    let len = 436.0_f64.sqrt();
    let (t, n) = ((-20.0 / len, -6.0 / len), (6.0 / len, -20.0 / len));
    // the round's contact circle about (20, 20) meets the side's contact line 8 in from it
    let h = 8.0 - 200.0 / len;
    let w = (4.0 - h * h).sqrt();
    let crossing = |sign: f64| {
        [
            20.0 + h * n.0 + sign * w * t.0,
            20.0 + h * n.1 + sign * w * t.1,
            10.0,
        ]
    };
    let nearest = |p: [f64; 3]| {
        let (_, brep) = kernel_copy(&b);
        brep.vertex_list()
            .iter()
            .map(|&v| {
                let q = solidcore::query::api_queries::vertex_get_point(&brep, v).unwrap();
                ((q.x - p[0]).powi(2) + (q.y - p[1]).powi(2) + (q.z - p[2]).powi(2)).sqrt()
            })
            .fold(f64::INFINITY, f64::min)
    };
    // on the arc's side of its end (x > 20): a vertex; past it: none
    assert!(
        nearest(crossing(-1.0)) < 1e-6,
        "{}",
        nearest(crossing(-1.0))
    );
    assert!(nearest(crossing(1.0)) > 1.0, "{}", nearest(crossing(1.0)));
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

/// An L of legs 40 and width 20, 10 high: its inner upright edge at (20, 20) is concave between
/// two flat faces and ends on the flat top and bottom, so a round can be carried on past
/// either face: each is named by its index, with the slab's frame at the edge's start, the
/// edge's 10 along it, and `along` × `across` the face's outward normal (+x or +y). A convex
/// edge has none; an index past the edges and a radius that is not positive are refused before
/// the kernel.
#[test]
fn the_faces_a_round_is_carried_on_past_are_the_kernels() {
    all_fixes();
    let l = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 20.0),
        (20.0, 20.0),
        (20.0, 40.0),
        (0.0, 40.0),
    ];
    let profile: Vec<_> = (0..l.len())
        .map(|i| segment(l[i], l[(i + 1) % l.len()]))
        .collect();
    let b = swept(&profile, 10.0);
    let inner = nearest_edge(&b, [20.0, 20.0, 5.0]);
    let found = solidcore_ops::carried_faces(&b, inner, 6.0).unwrap();
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].face, found[1].face);
    let mut normals = Vec::new();
    for c in &found {
        assert!(c.face < b.to_record().unwrap().0.faces.len());
        assert!((c.origin[0] - 20.0).abs() < 1e-9 && (c.origin[1] - 20.0).abs() < 1e-9);
        assert_eq!(
            c.outline,
            [
                [0.0, -c.reach],
                [10.0, -c.reach],
                [10.0, c.reach],
                [0.0, c.reach]
            ]
        );
        assert_eq!(c.depth, [-c.reach, 0.0]);
        let (a, x) = (
            Vec3::new(c.along[0], c.along[1], c.along[2]),
            Vec3::new(c.across[0], c.across[1], c.across[2]),
        );
        normals.push(a.cross(x));
    }
    let is = |n: Vec3, x: f64, y: f64| (n - Vec3::new(x, y, 0.0)).length() < 1e-9;
    assert!(normals.iter().any(|&n| is(n, 1.0, 0.0)) && normals.iter().any(|&n| is(n, 0.0, 1.0)));
    let convex = nearest_edge(&b, [40.0, 0.0, 5.0]);
    assert!(solidcore_ops::carried_faces(&b, convex, 6.0)
        .unwrap()
        .is_empty());
    assert!(matches!(
        solidcore_ops::carried_faces(&b, 999, 6.0),
        Err(Error::Argument)
    ));
    assert!(matches!(
        solidcore_ops::carried_faces(&b, inner, 0.0),
        Err(Error::Argument)
    ));
}

/// The kernel rig's fix set (`fixes.json`), as the rig runs its entries.
fn rig_fixes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fixes.json"
    );
    let fixes: Vec<String> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    solidcore_ops::session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
}

/// The ledge of the kernel case `10876-fillet-ledge-convex-and-concave-edges-at-bore-r0-5`:
/// its operand body `b255`.
fn ledge() -> Body {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fillets/",
        "10876-fillet-ledge-convex-and-concave-edges-at-bore-r0-5.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b255"].clone()).unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The ledge's concave edge rounded first (r 0.5, alone: it ends flush on the bore's flat
/// wall x = 12), then its convex edge's remainder with the concave round's end rib on that
/// wall rounded as one chain: the second pass's trim leaks past its rails (the convex ball
/// would have to end where the chain runs into the sharp edge it leaves unselected). The
/// refusal keeps the kernel's class and statement, not a bare status 1001.
#[test]
fn a_ledge_rounded_concave_first_refuses_its_convex_chain_with_its_reason() {
    rig_fixes();
    let b = ledge();
    let concave = nearest_edge(&b, [15.0, 30.0, 33.0]);
    blend(&b, Blend::Round, &[concave], &[0.5], &[], Corners::Filled).unwrap();
    let h = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [12.0, 29.5 + h, 33.5 - h]),
        nearest_edge(&b, [12.0, 25.0, 33.0]),
    ];
    assert_ne!(chain[0], chain[1]);
    let got = blend(&b, Blend::Round, &chain, &[0.5], &[], Corners::Filled);
    match got {
        Err(Error::BlendFailed { status, phrase, .. }) => {
            assert_eq!(status, 1001);
            assert!(
                phrase.contains("the faces the rails enclose reach past the rails"),
                "{phrase}"
            );
        }
        other => panic!("not the trim's typed refusal: {other:?}"),
    }
}

/// The ledge's planar twin (#10876): the slab [0, 10] x [-10, 10] x [-10, 0] with the riser
/// [0, 10] x [0, 10] x [0, 10] on it. Their walls x = 0 are one face, so three edges meet at
/// V = 0: E1 (the top z = 0 and the wall, convex), E2 (the top and the riser's front y = 0,
/// concave) and the wall's edge with the riser's front (convex).
fn stair_twin() -> Body {
    let slab = make_box([0.0, -10.0, -10.0], [10.0, 20.0, 10.0]).unwrap();
    let riser = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    solidcore_ops::boolean::boolean(slab, riser, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// The `n` Gauss-Legendre nodes and weights on [-1, 1]: each node the root of the Legendre
/// polynomial `P_n` from its Chebyshev estimate by Newton's method (the three-term recurrence
/// for `P_n` and its derivative), to machine precision.
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 1..=n {
        let mut x = (PI * (i as f64 - 0.25) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for j in 2..=n {
                let p2 = ((2 * j - 1) as f64 * x * p1 - (j - 1) as f64 * p0) / j as f64;
                p0 = p1;
                p1 = p2;
            }
            dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            let step = p1 / dp;
            x -= step;
            if step.abs() < 1e-16 {
                break;
            }
        }
        out.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
    }
    out
}

/// `f` integrated over `[a, b]` by the `n`-point Gauss-Legendre rule on `panels` equal panels.
fn gauss(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize, panels: usize) -> f64 {
    let rule = gauss_legendre(n);
    let h = (b - a) / panels as f64;
    let mut sum = 0.0;
    for k in 0..panels {
        let (lo, hi) = (a + h * k as f64, a + h * (k + 1) as f64);
        for &(x, w) in &rule {
            sum += w * 0.5 * (hi - lo) * f(0.5 * (lo + hi) + 0.5 * (hi - lo) * x);
        }
    }
    sum
}

/// The material the blend corner's Roll-over adds on the stair twin (E1 rounded `r1`, E2
/// `r2`), past E2's fillet strip: about E1's axis A1 (x = r1, z = -r1, along y), the concave
/// ball's region for each direction `theta` from +z towards -x over the quarter turn the round
/// spans, in the meridian (distance `rho` from A1, height along y) the fillet section between
/// the round (`rho = r1`), the riser's front (y = 0) and the ball (centre `rho = R = r1 + r2`,
/// y = -r2): of height `r2 - sqrt(r2^2 - (R - rho)^2)` at `rho`. The cliff x = 0 keeps
/// `rho sin(theta) <= r1`, so `theta` up to `asin(r1 / rho)`:
/// `I = int_r1^R rho (r2 - sqrt(r2^2 - (R - rho)^2)) asin(r1 / rho) d rho`, and with
/// `rho = R - r2 cos(phi)` (analytic in `phi` over `[0, pi/2]`)
/// `I = int_0^(pi/2) rho r2^2 (1 - sin phi) sin phi asin(r1 / rho) d phi`. Integrated by two
/// Gauss-Legendre rules (20 points on 8 panels, 30 points on 16) that agree to 1e-14
/// relative (asserted; measured 9e-16, the rounding of summing their 160 and 480 terms), a
/// thousand times under the volume tests' 1e-11: the reference is their mean.
fn roll_over_material(r1: f64, r2: f64) -> f64 {
    let big_r = r1 + r2;
    let f = |phi: f64| {
        let rho = big_r - r2 * phi.cos();
        rho * r2 * r2 * (1.0 - phi.sin()) * phi.sin() * (r1 / rho).min(1.0).asin()
    };
    let a = gauss(&f, 0.0, PI / 2.0, 20, 8);
    let b = gauss(&f, 0.0, PI / 2.0, 30, 16);
    assert!((a - b).abs() <= 1e-14 * a.abs(), "{a} {b}");
    0.5 * (a + b)
}

/// The stair twin's volume once E1 is rounded `r1` and E2 `r2` as the blend corner's
/// Roll-over: 3000, less E1's strip `(1 - pi/4) r1^2` over its length 10, plus E2's fillet
/// strip `(1 - pi/4) r2^2` over its remainder from x = r1 to 10, plus the material the trimmed
/// torus adds ([`roll_over_material`]). At r1 = r2 = 1: 3000.047071660769.
fn rolled_volume(r1: f64, r2: f64) -> f64 {
    let strip = 1.0 - PI / 4.0;
    3000.0 - 10.0 * strip * r1 * r1 + (10.0 - r1) * strip * r2 * r2 + roll_over_material(r1, r2)
}

/// The kernel's certified enclosure of `b`'s volume holds `want`, and is narrower than
/// `width` (so that it decides `want` against a nearby value).
fn enclosure_holds(b: &Body, want: f64, width: f64) {
    let (k, mut brep) = kernel_copy(b);
    let e = solidcore::mass::enclosure::enclose_volume(&k, &mut brep, width / 4.0).unwrap();
    assert!(e.lo <= want && want <= e.hi, "{} {want} {}", e.lo, e.hi);
    assert!(e.hi - e.lo <= width, "{} {}", e.lo, e.hi);
}

/// The torus face's edges of `brep` are exactly `ends`, each the two points its edge runs
/// between (either way round, each within 1e-9) and the kind of the face beside it.
fn torus_edges_are(brep: &solidcore::Brep, torus: FaceId, ends: &[([f64; 3], [f64; 3], &str)]) {
    let edges = solidcore::topology::face::p4::edges(brep, torus, None, false, None).unwrap();
    assert_eq!(edges.len(), ends.len(), "the torus face's edges");
    let near = |p: Vec3, q: [f64; 3]| (p - Vec3::new(q[0], q[1], q[2])).length() <= 1e-9;
    for &(a, b, kind) in ends {
        let found = edges.iter().any(|&e| {
            let v0 = edge::start_vertex(brep, e).unwrap().unwrap();
            let v1 = edge::end_vertex(brep, e).unwrap().unwrap();
            let (p, q) = (
                brep.vertex(v0).unwrap().point,
                brep.vertex(v1).unwrap().point,
            );
            let faces = edge::faces(brep, e, None).unwrap();
            let other = *faces.iter().find(|&&f| f != torus).unwrap();
            let beside = surface_kind_name(brep.face(other).unwrap().surface.as_ref().unwrap());
            ((near(p, a) && near(q, b)) || (near(p, b) && near(q, a))) && beside == kind
        });
        assert!(found, "no torus edge {a:?} - {b:?} beside a {kind}");
    }
}

/// The twin's convex edge E1 rounded at r = 1 alone (it ends on the riser's front continued,
/// its end cap a spandrel on that plane), then the concave chain it leaves, E2's remainder and
/// E1's end arc on the riser's front, rounded at r = 1 on its own: the blend corner's
/// Roll-over's second pass, asked as a feature of its own.
fn stair_twin_rolled() -> Body {
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    assert_ne!(chain[0], chain[1]);
    blend(&b, Blend::Round, &chain, &[1.0], &[], Corners::Filled).unwrap();
    b
}

/// The twin's torus face, with its torus's centre, axis and radii: the one face on a torus
/// (an exact torus, or a surface of revolution of a circular arc, read off its arc; the test
/// below proves the face on that torus through its normals); its other faces planes and
/// cylinders.
fn rolled_torus(brep: &solidcore::Brep) -> (FaceId, (Vec3, Vec3, f64, f64)) {
    let mut tori = Vec::new();
    for &f in brep.face_list() {
        let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
        if let Some(t) =
            solidcore::surface::procedural_blend::exact::revolved_circle_torus(surface).unwrap()
        {
            tori.push((f, t));
            continue;
        }
        match surface.kind() {
            SurfaceKind::Torus(t) => {
                let place = t.base().position();
                tori.push((
                    f,
                    (
                        place.origin(),
                        place.z_axis(),
                        t.major_radius(),
                        t.minor_radius(),
                    ),
                ));
            }
            SurfaceKind::Plane(_) => {}
            SurfaceKind::Cone(c) => assert!(c.is_cylinder(), "a face on a cone"),
            _ => panic!("a face on no exact surface: {}", surface_kind_name(surface)),
        }
    }
    assert_eq!(tori.len(), 1, "one torus face");
    tori[0]
}

/// The exact unit normal out of the rolled twin at `p` on its torus (towards the concave
/// ball's centre, which lies on the circle of radius 2 about E1's axis x = 1, z = -1, in the
/// plane y = -1), with `p`'s distance `rho` from that axis.
fn torus_normal(p: Vec3) -> (Vec3, f64) {
    let (dx, dz) = (p.x - 1.0, p.z + 1.0);
    let rho = (dx * dx + dz * dz).sqrt();
    let centre = Vec3::new(1.0 + 2.0 * dx / rho, -1.0, -1.0 + 2.0 * dz / rho);
    ((centre - p).unitize().unwrap(), rho)
}

#[test]
fn a_stair_rounded_convex_first_rolls_its_concave_chain_over_the_round() {
    let b = stair_twin_rolled();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let v = b.volume(1e-12).unwrap();
    let want = rolled_volume(1.0, 1.0);
    assert!(rel(v, want) < 1e-11, "{v}");
    enclosure_holds(&b, want, 1e-5);
    let (k, mut brep) = kernel_copy(&b);
    let (torus, (o, z, major, minor)) = rolled_torus(&brep);
    assert!(
        (major - 2.0).abs() < 1e-12 && (minor - 1.0).abs() < 1e-12,
        "{major} {minor}"
    );
    assert!(
        (o.x - 1.0).abs() < 1e-12 && (o.z + 1.0).abs() < 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() < 1e-12 && z.z.abs() < 1e-12, "{z:?}");
    // its four edges: the crease with the wall x = 0, from where the rail leaves the riser's
    // front (rho = 2: z = sqrt 3 - 1) to the tangent point on the round (rho = 1, in the balls'
    // centres' plane y = -1); the rail on the riser's front, the circle rho = 2 at y = 0 from
    // there to E2's fillet (1, 0, 1); the seam with E2's fillet in the plane x = 1; and the
    // seam with E1's round in the balls' centres' plane y = -1
    let lift = 3f64.sqrt() - 1.0;
    torus_edges_are(
        &brep,
        torus,
        &[
            ([0.0, 0.0, lift], [0.0, -1.0, -1.0], "plane"),
            ([0.0, 0.0, lift], [1.0, 0.0, 1.0], "plane"),
            ([1.0, -1.0, 0.0], [1.0, 0.0, 1.0], "cone"),
            ([0.0, -1.0, -1.0], [1.0, -1.0, 0.0], "cone"),
        ],
    );
    for e in solidcore::topology::face::p4::edges(&brep, torus, None, false, None).unwrap() {
        let faces = edge::faces(&brep, e, None).unwrap();
        let other = *faces.iter().find(|&&f| f != torus).unwrap();
        let wall = match brep.face(other).unwrap().surface.as_ref().unwrap().kind() {
            SurfaceKind::Plane(p) => p.is_planar().1.x.abs() > 1.0 - 1e-12,
            _ => false,
        };
        let ivl = brep.edge(e).unwrap().interval;
        for i in 1..8 {
            let s = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 8.0;
            let frame = |f: FaceId, brep: &mut solidcore::Brep| {
                let eu = edge::edgeuse_of_face(brep, e, Some(f)).unwrap().unwrap();
                let fu = solidcore::topology::face::p2::outer_faceuse(brep, f)
                    .unwrap()
                    .unwrap();
                let frame =
                    solidcore::query::api_queries::edgeuse_face_frame(&k, brep, eu, s, fu).unwrap();
                let at = brep
                    .face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .unwrap()
                    .evaluate_point(&k, frame.uv)
                    .unwrap();
                (frame.normal, at)
            };
            // the exact normal where the torus face's normal is taken: its surface's point at
            // the edge use's (u, v)
            let (on_torus, p) = frame(torus, &mut brep);
            let (exact, rho) = torus_normal(p);
            assert!(
                (on_torus.value - exact).length() <= 1e-9 + on_torus.bound,
                "{p:?} {:?} {exact:?} {} wall {wall}",
                on_torus.value,
                on_torus.bound
            );
            if wall {
                let cos_p = 2.0 - rho;
                let cos_a = on_torus.value.x.abs();
                assert!((cos_a - cos_p / (2.0 - cos_p)).abs() <= 1e-9, "{p:?}");
            } else {
                let (beside, q) = frame(other, &mut brep);
                // the face beside meets the torus where the edge lies: the same exact normal,
                // to the edge's own gap between its two faces' points
                let gap = (q - p).length();
                assert!(
                    (beside.value - exact).length() <= 1e-9 + beside.bound + gap,
                    "{p:?}"
                );
            }
        }
    }
}

/// A nearby corner that is not the Roll-over still refuses as before: the riser runs on past
/// the wall over the void (the riser [-5, 10] x [0, 10] x [0, 10] on the slab), so that at V
/// the edge beyond the seam is the riser's front on its underside (no cliff rim) and the slab's
/// wall meets the riser's underside at a concave edge (no tangent seam). The vertex has four
/// edges, a convex and a concave round meeting there, and is refused as before.
#[test]
fn a_stair_whose_riser_overhangs_the_wall_still_refuses_its_corner() {
    rig_fixes();
    let slab = make_box([0.0, -10.0, -10.0], [10.0, 20.0, 10.0]).unwrap();
    let riser = make_box([-5.0, 0.0, 0.0], [15.0, 10.0, 10.0]).unwrap();
    let b = solidcore_ops::boolean::boolean(slab, riser, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    let got = blend(&b, Blend::Round, &[e1, e2], &[1.0], &[], Corners::Filled);
    assert!(
        matches!(&got, Err(Error::Unsupported(r)) if r.contains("vertex of four edges")),
        "{:?}",
        got.err()
    );
}

/// The stair twin with its wall split along z = 0 under the riser (the wall imprinted with the
/// box [-5, 0] x [0, 10] x [0, 10]): four edges meet at V = 0, as at the ledge's corner (#10876):
/// E1 (the top and the slab's wall, convex), E2 (the top and the riser's front, concave), the
/// tangent seam between the two walls, and the riser's wall's edge with its front (the cliff's
/// rim, sharp and convex).
fn stair_twin_of_four_edges() -> Body {
    let tool = make_box([-5.0, 0.0, 0.0], [5.0, 10.0, 10.0]).unwrap();
    let (b, changed) = solidcore_ops::boolean::imprint(stair_twin(), tool).unwrap();
    assert!(changed);
    let (record, _) = b.to_record().unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The four-edge twin's E1 rounded at 1 and E2 at 0.4 in one call: the corners step refuses
/// V as a convex and a concave round meeting at four edges, notes it as the blend corner's
/// Roll-over, and builds it in two passes (E1 first, then E2's remainder with E1's end arc on
/// the riser's front as one concave chain). At r2 = 0.4 the torus's rail on the riser's front
/// (rho = R = 1.4 about E1's axis) leaves it across the slab's wall below V (at
/// z = sqrt(R^2 - 1) - 1 < 0), so the End lies on one cliff face. The result is valid
/// (level 2), its volume is the closed form's (the certified enclosure holds it), its one
/// torus has radii 1.4 and 0.4 about E1's axis, and its four edges are the crease with the
/// wall, the rail on the riser's front and the seams with the two rounds.
#[test]
fn a_ledge_twin_of_four_edges_rolls_over_in_one_call() {
    rig_fixes();
    let (r1, r2) = (1.0, 0.4);
    let b = stair_twin_of_four_edges();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    blend(&b, Blend::Round, &[e1, e2], &[r1, r2], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let want = rolled_volume(r1, r2);
    let v = b.volume(1e-12).unwrap();
    assert!(rel(v, want) < 1e-11, "{v} {want}");
    enclosure_holds(&b, want, 1e-5);
    let (_, brep) = kernel_copy(&b);
    let (torus, (o, z, major, minor)) = rolled_torus(&brep);
    assert!(
        (major - (r1 + r2)).abs() < 1e-12 && (minor - r2).abs() < 1e-12,
        "{major} {minor}"
    );
    assert!(
        (o.x - r1).abs() < 1e-12 && (o.y + r2).abs() < 1e-12 && (o.z + r1).abs() < 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() < 1e-12 && z.z.abs() < 1e-12, "{z:?}");
    let exit = ((r1 + r2) * (r1 + r2) - r1 * r1).sqrt() - r1;
    torus_edges_are(
        &brep,
        torus,
        &[
            ([0.0, 0.0, exit], [0.0, -r2, -r1], "plane"),
            ([0.0, 0.0, exit], [r1, 0.0, r2], "plane"),
            ([r1, -r2, 0.0], [r1, 0.0, r2], "cone"),
            ([0.0, -r2, -r1], [r1, -r2, 0.0], "cone"),
        ],
    );
}

/// The four-edge twin rounded at 1 and 1 in one call: the corner is the Roll-over's, but its
/// two passes do not build (the rail on the riser's front leaves it across the riser's wall,
/// above V, and the End's rib would span both cliff faces), so the call falls back to the
/// first call's refusal, the corner's four-edge reason, and leaves the body as it was.
#[test]
fn a_ledge_twin_whose_roll_over_does_not_build_keeps_its_refusal() {
    rig_fixes();
    let b = stair_twin_of_four_edges();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    let got = blend(&b, Blend::Round, &[e1, e2], &[1.0], &[], Corners::Filled);
    assert!(
        matches!(&got, Err(Error::Unsupported(r)) if r.contains("vertex of four edges")),
        "{:?}",
        got.err()
    );
    assert!(rel(b.volume(1e-12).unwrap(), 3000.0) < 1e-12);
}

/// A block 30 x 30 x 10 with the pocket [5, 25]^2 x [4, 10], its four vertical edges rounded
/// `RC`: its floor's edges are four lines and four arcs, each concave, each arc between a
/// corner's cylinder and the floor (square to the cylinder's axis), each meeting the next
/// edge G1.
const RC: f64 = 8.0;
fn rounded_pocket() -> Body {
    let block = make_box([0.0, 0.0, 0.0], [30.0, 30.0, 10.0]).unwrap();
    let cut = make_box([5.0, 5.0, 4.0], [20.0, 20.0, 10.0]).unwrap();
    let b = solidcore_ops::boolean::boolean(block, cut, solidcore_ops::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let verticals: Vec<usize> = [[5.0, 5.0], [25.0, 5.0], [25.0, 25.0], [5.0, 25.0]]
        .iter()
        .map(|p| nearest_edge(&b, [p[0], p[1], 7.0]))
        .collect();
    blend(&b, Blend::Round, &verticals, &[RC], &[], Corners::Filled).unwrap();
    b
}

/// The rounded pocket's floor loop: its four lines, then its four arcs.
fn pocket_floor(b: &Body) -> Vec<usize> {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut out = Vec::new();
    for p in [[15.0, 5.0], [25.0, 15.0], [15.0, 25.0], [5.0, 15.0]] {
        out.push(nearest_edge(b, [p[0], p[1], 4.0]));
    }
    for (cx, cy, dx, dy) in [
        (5.0 + RC, 5.0 + RC, -1.0, -1.0),
        (25.0 - RC, 5.0 + RC, 1.0, -1.0),
        (25.0 - RC, 25.0 - RC, 1.0, 1.0),
        (5.0 + RC, 25.0 - RC, -1.0, 1.0),
    ] {
        out.push(nearest_edge(b, [cx + dx * RC * s, cy + dy * RC * s, 4.0]));
    }
    out
}

/// A pocket floor with rounded vertical corners is not the Roll-over's concave chain: at each
/// end of a corner's arc on the floor, the floor's next edge runs on concave (G1 into the next
/// wall's foot), never sharp and convex as the ledge's cliff rim is. So the gate refuses its
/// loop and any part of it, while the stair twin's chain (E2's remainder with E1's end arc on
/// the riser's front) passes it with its one V, the arc's end on the wall at (0, 0, -1).
#[test]
fn only_the_ledges_chain_is_the_roll_overs_second_pass() {
    use solidcore::blend::blend_corner::rollover::{concave_chain_ends, is_concave_chain};
    rig_fixes();
    let b = rounded_pocket();
    let floor = pocket_floor(&b);
    let (k, mut brep) = kernel_copy(&b);
    let ids = |brep: &solidcore::Brep, picks: &[usize]| -> Vec<EdgeId> {
        picks.iter().map(|&i| brep.edge_list()[i]).collect()
    };
    let all = ids(&brep, &floor);
    let part = ids(&brep, &[floor[0], floor[4], floor[5]]);
    for edges in [&all, &part] {
        assert!(!is_concave_chain(&k, &mut brep, edges).unwrap());
        assert!(concave_chain_ends(&k, &mut brep, edges).unwrap().is_empty());
    }
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let (k, mut brep) = kernel_copy(&b);
    let chain = ids(&brep, &chain);
    assert!(is_concave_chain(&k, &mut brep, &chain).unwrap());
    let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
    assert_eq!(ends.len(), 1);
    let p = brep.vertex(ends[0]).unwrap().point;
    assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-12, "{p:?}");
}

/// The rounded pocket's floor loop rounded at 1 is rounded as before (not as the Roll-over's
/// second pass): valid, and its volume the closed form's. The block less the pocket is
/// 9000 - 2400; each corner's round adds `(1 - pi/4) RC^2` over the wall's height 6; the
/// floor's round adds its section `A = (1 - pi/4) r^2` along the four lines (each
/// `20 - 2 RC` long) and, by Pappus, `A (pi / 2) (RC - c)` round each corner, `c` the section's
/// centroid's distance from the wall, `r (5/6 - pi/4) / (1 - pi/4)` (the square's moment less
/// the quarter disc's about the wall, over the area).
#[test]
fn a_pocket_floor_with_rounded_corners_is_rounded_as_before() {
    rig_fixes();
    let r = 1.0;
    let b = rounded_pocket();
    let floor = pocket_floor(&b);
    blend(&b, Blend::Round, &floor, &[r], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let strip = 1.0 - PI / 4.0;
    let section = strip * r * r;
    let centroid = r * (5.0 / 6.0 - PI / 4.0) / strip;
    let want = 9000.0 - 2400.0
        + 4.0 * strip * RC * RC * 6.0
        + 4.0 * section * (20.0 - 2.0 * RC)
        + 4.0 * section * (PI / 2.0) * (RC - centroid);
    let v = b.volume(1e-12).unwrap();
    assert!(rel(v, want) < 1e-11, "{v} {want}");
}

/// The rounded pocket's floor loop rounded at 7, past the walls' height 6, is refused as
/// before: too large, its rail leaving the wall it rolls on over the pocket's rim (a concave
/// round adding material, never built as the Roll-over's trimmed cliff overflow), with a
/// largest size that builds under the walls' height; the body is left as it was.
#[test]
fn a_pocket_floor_round_past_its_walls_keeps_its_refusal() {
    rig_fixes();
    let b = rounded_pocket();
    let before = b.volume(1e-12).unwrap();
    let floor = pocket_floor(&b);
    let got = blend(&b, Blend::Round, &floor, &[7.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 5.9 && size < 6.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// In the Roll-over's second pass only the corner at V (where the concave chain meets the
/// cliff) looks for its rail ends across the cliff's tangent seam: on the stair twin's chain
/// (E1 rounded first), the corner at V = (0, 0, -1) carries the Roll-over's search and the
/// chain's other end, on the side face x = 10, does not (its rail ends are found as in any
/// pass, a rail end just past its side edge's end kept where the solve put it).
#[test]
fn only_the_corner_at_v_looks_across_the_cliffs_seam() {
    use solidcore::blend::blend_corner::rollover::concave_chain_ends;
    use solidcore::blend::fillet_executive::session::FilletSession;
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let picks = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let (mut k, mut brep) = kernel_copy(&b);
    let chain: Vec<EdgeId> = picks.iter().map(|&i| brep.edge_list()[i]).collect();
    let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
    assert_eq!(ends.len(), 1);
    let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &chain, 1.0, 1, 1, 1.0)
        .unwrap();
    session.set_roll_over_pass(ends.clone());
    session.create_corners(&k, None).unwrap();
    let (mut at_v, mut far_end) = (0, 0);
    for id in session.fillet_corners().unwrap() {
        let corner = session.corner(id).unwrap();
        let vertex = corner.filleted_vertex().unwrap();
        assert_eq!(corner.roll_over, vertex == ends[0], "{vertex:?}");
        at_v += usize::from(vertex == ends[0]);
        far_end += usize::from(!corner.roll_over);
    }
    assert_eq!(at_v, 1, "a corner at V");
    assert!(far_end >= 1, "a corner away from V");
}

/// The stair twin's concave chain (E1 rounded at 1 first) rounded at 12, past the riser's
/// height 10: E2's remainder overflows the riser's top rim, away from V, so even as the
/// Roll-over's second pass (tried on the first call's refusal) that concave overflow is
/// refused (only a blend whose edge ends at V overflows as the event's). The call keeps the
/// first call's refusal, too large with its rail leaving the face, at a size under the riser's
/// height, and leaves the body as it was.
#[test]
fn an_oversized_ledge_chain_keeps_its_refusal() {
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let before = b.volume(1e-12).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let got = blend(&b, Blend::Round, &chain, &[12.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 0.0 && size < 10.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// The stair twin with its riser notched beside the wall (the box [0, 0.95] x [0, 10] x
/// [0.9, 10] taken away): E1 rounded at 1 first, then its concave chain at 1. E2's remainder
/// (x >= 1) keeps its rail on the riser's front (z = 1 under the height 10), but E1's end arc
/// at V rolls its rail (the circle of radius 2 about E1's axis on y = 0) above the notch's
/// floor z = 0.9 for 0.38 < x < 0.95: it leaves the riser's front across the notch's edges,
/// away from the cliff's rim at V. That concave overflow is not the Roll-over's, so the second
/// pass refuses it and the call keeps the first call's refusal (too large, its rail leaving
/// the face), the body as it was.
#[test]
fn a_ledge_chain_whose_arc_overflows_away_from_the_cliff_keeps_its_refusal() {
    rig_fixes();
    let notch = make_box([0.0, 0.0, 0.9], [0.95, 10.0, 9.1]).unwrap();
    let b =
        solidcore_ops::boolean::boolean(stair_twin(), notch, solidcore_ops::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let before = b.volume(1e-12).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let got = blend(&b, Blend::Round, &chain, &[1.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 0.0 && size < 1.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// The kernel's decision itself, in the Roll-over's second pass: the stair twin's chain
/// overflows the riser's front only across the cliff's rim at V, so its concave overflow is
/// the event's (the session builds); the notched twin's arc, though its edge ends at
/// V, leaves the front across the notch's edges, away from the rim, so its overflow is refused
/// as a `ConcaveOverflow`, as on main.
#[test]
fn only_an_overflow_across_the_cliffs_rim_at_v_is_the_roll_overs() {
    use solidcore::blend::blend_corner::rollover::concave_chain_ends;
    use solidcore::blend::fillet_executive::p3::NoRoomReason;
    use solidcore::blend::fillet_executive::session::FilletSession;
    rig_fixes();
    let notched = {
        let notch = make_box([0.0, 0.0, 0.9], [0.95, 10.0, 9.1]).unwrap();
        solidcore_ops::boolean::boolean(stair_twin(), notch, solidcore_ops::BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    };
    let mut reasons = Vec::new();
    for b in [stair_twin(), notched] {
        let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
        blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let picks = [
            nearest_edge(&b, [5.0, 0.0, 0.0]),
            nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
        ];
        let (mut k, mut brep) = kernel_copy(&b);
        let chain: Vec<EdgeId> = picks.iter().map(|&i| brep.edge_list()[i]).collect();
        let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
        assert_eq!(ends.len(), 1);
        let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &chain, 1.0, 1, 1, 1.0)
            .unwrap();
        session.set_roll_over_pass(ends);
        let done = session.do_fillet(&mut k);
        reasons.push((done.is_ok(), session.error_info().no_room_reason()));
    }
    assert_eq!(reasons[0], (true, None), "{reasons:?}");
    assert_eq!(
        reasons[1],
        (false, Some(NoRoomReason::ConcaveOverflow)),
        "{reasons:?}"
    );
}

/// A session's failure says what the session noted of the step that failed
/// ([`solidcore_ops::blend::session_failure`]). The session here is asked to fillet with no
/// edge set up: it makes no fillet face, and what it noted says so after the kernel's statement.
#[test]
fn a_session_failure_says_what_its_session_noted() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    let (mut k, mut brep) = kernel_copy(&block());
    let mut s = FilletSession::new(&mut k, &mut brep).unwrap();
    let e = s.do_filleting(&mut k, None).unwrap_err();
    assert_eq!(
        blend::failure(&e).unwrap().to_string(),
        "the blend failed (status 1001): the fillet body has no faces"
    );
    assert_eq!(
        blend::session_failure(&s, &e).unwrap().to_string(),
        "the blend failed (status 1001): the fillet body has no faces: Fillet Surface \
         Computation Failure: no fillet faces were made"
    );
}

/// tangency-labs/solidcraft#12175: a ring (radii 17.5 and 7.5, z -10 to 10) joined by a tab
/// 20 wide (x 10 to 60, z -5 to 5), its edges at the ring and its long edges rounded r 2, a part
/// mirror symmetric about z = 0. A round on the ring's outer circle builds alike at the bottom
/// and at the top. It runs the whole closed edge, so its end section is its start one, but the
/// two are solved apart: at the bottom the turn measured between them came out a rounding short
/// of 360 degrees, a sweep SMLib's check refuses (`SmSurfOfRevolution` Test #7), at r 0.14 and 2
/// here (r 0.13 rounds the other way). Each round takes off the ring's corner, by Pappus
/// 2 pi r^2 ((R - r)(1 - pi/4) + r/6), and its face is the torus R - r, r about the ring's axis.
/// A chamfer round the circle, a cone, builds alike at both ends too.
#[test]
fn a_round_along_a_whole_circle_builds_alike_on_both_ends_of_a_mirrored_ring() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::BooleanOp;

    all_fixes();
    let (outer, inner, half) = (17.5_f64, 7.5, 10.0_f64);
    let circle = |radius: f64, axis: f64, start: f64| Curve::Arc {
        center: [0.0, 0.0, -10.0],
        axis: [0.0, 0.0, axis],
        reference: [1.0, 0.0, 0.0],
        radius,
        start,
        end: start + 2.0 * PI,
    };
    let ring = extrude(
        &[circle(outer, 1.0, 0.0), circle(inner, -1.0, -2.0 * PI)],
        [0.0, 0.0, 1.0],
        20.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let corners = [[10.0, -half], [60.0, -half], [60.0, half], [10.0, half]];
    let outline: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], a[1], -5.0],
                end: [b[0], b[1], -5.0],
            }
        })
        .collect();
    let tab = extrude(
        &outline,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let base = boolean(ring, tab, BooleanOp::Union).unwrap().unwrap();
    // the tab's two upright edges and four arcs at the ring, and its four long edges
    let x = (outer * outer - half * half).sqrt();
    let mid = (half / outer).asin() / 2.0;
    let (ax, ay) = (outer * mid.cos(), outer * mid.sin());
    let mut at = vec![[x, -half, 0.0], [x, half, 0.0]];
    for z in [-5.0, 5.0] {
        at.extend([[ax, -ay, z], [ax, ay, z], [35.0, -half, z], [35.0, half, z]]);
    }
    let edges: Vec<usize> = at.iter().map(|&p| edge_at(&base, p)).collect();
    blend(&base, Blend::Round, &edges, &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(base.counts().unwrap(), [23, 53, 32]);
    let before = base.volume(1e-10).unwrap();
    // each face's kind and a revolution's sweep (degrees), and for a whole turn, how far a
    // 9 x 9 grid of its points lies from the torus of a round r on the circle at z (an ops body
    // reads back through its .smb, which keeps a round's form, not its definition: no
    // procedural_blend() to read here)
    let faces = |b: &Body, r: f64, z: f64| {
        let (k, made) = kernel_copy(b);
        let centre_z = z - r * z.signum();
        let mut off = 0.0_f64;
        let mut kinds: Vec<(&'static str, Option<[f64; 2]>)> = Vec::new();
        for &f in made.face_list() {
            let s = made.face(f).unwrap().surface.as_ref().unwrap();
            let sweep = match s.kind() {
                SurfaceKind::SurfOfRevolution(rev) => {
                    let step = rev.step_uv_domain();
                    let u = step.u_interval();
                    if u.max() - u.min() == 360.0 {
                        for i in 0..9 {
                            for j in 0..9 {
                                let uv = step.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0);
                                let p = s.evaluate_step_point(&k, uv).unwrap();
                                let rho = p.x.hypot(p.y) - (outer - r);
                                off = off.max((rho.hypot(p.z - centre_z) - r).abs());
                            }
                        }
                    }
                    Some([u.min(), u.max()])
                }
                _ => None,
            };
            kinds.push((surface_kind_name(s), sweep));
        }
        kinds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (kinds, off)
    };
    let (kinds_before, _) = faces(&base, 2.0, 10.0);
    // the tab's four rounds along the ring's arcs keep their part turns: only a whole closed
    // edge's round turns 360
    let part_turns: Vec<f64> = kinds_before
        .iter()
        .filter_map(|&(_, sweep)| sweep.map(|[lo, hi]| hi - lo))
        .collect();
    assert_eq!(part_turns.len(), 4, "{part_turns:?}");
    assert!(part_turns.iter().all(|&t| t < 90.0), "{part_turns:?}");
    for r in [0.14, 2.0] {
        let corner = 2.0 * PI * r * r * ((outer - r) * (1.0 - PI / 4.0) + r / 6.0);
        for z in [10.0, -10.0] {
            let b = base.copy().unwrap();
            let edge = edge_at(&b, [-outer, 0.0, z]);
            blend(&b, Blend::Round, &[edge], &[r], &[], Corners::Filled)
                .unwrap_or_else(|e| panic!("r {r} at z {z}: {e:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "r {r} at z {z}: {}", report.text);
            assert_eq!(b.counts().unwrap(), [24, 55, 33], "r {r} at z {z}");
            let v = b.volume(1e-10).unwrap();
            assert!(
                rel(v, before - corner) < 1e-9,
                "r {r} at z {z}: {v}, want {}",
                before - corner
            );
            let (mut added, off) = faces(&b, r, z);
            for k in &kinds_before {
                let i = added.iter().position(|a| a == k).unwrap();
                added.remove(i);
            }
            assert_eq!(
                added,
                vec![("surface-of-revolution", Some([0.0, 360.0]))],
                "r {r} at z {z}"
            );
            assert!(off < 1e-9, "r {r} at z {z}: {off} off the torus");
        }
    }
    // a chamfer d 2 round the circle (a linear section revolved, stored as its cone) takes off
    // the triangle d^2/2 whose centroid turns at R - d/3: pi d^2 (R - d/3)
    let d = 2.0;
    for z in [10.0, -10.0] {
        let b = base.copy().unwrap();
        let edge = edge_at(&b, [-outer, 0.0, z]);
        blend(&b, Blend::Chamfer, &[edge], &[d], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("chamfer at z {z}: {e:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "chamfer at z {z}: {}", report.text);
        assert_eq!(b.counts().unwrap(), [24, 55, 33], "chamfer at z {z}");
        let v = b.volume(1e-10).unwrap();
        let want = before - PI * d * d * (outer - d / 3.0);
        assert!(rel(v, want) < 1e-9, "chamfer at z {z}: {v}, want {want}");
        let (mut added, _) = faces(&b, d, z);
        for k in &kinds_before {
            let i = added.iter().position(|a| a == k).unwrap();
            added.remove(i);
        }
        assert_eq!(added, vec![("cone", None)], "chamfer at z {z}");
    }
}

/// A revolved r8 corner, clipped on x = +/-10. Its torus support must survive the
/// round's definition record and reload, including the partial generator's parameters.
#[test]
fn a_round_on_a_revolved_torus_end_edge_survives_reload() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;

    all_fixes();
    let radial = [-3.0_f64.sqrt() / 2.0, 0.5, 0.0];
    let at = |rho: f64, z: f64| [radial[0] * rho, radial[1] * rho, z];
    let profile = [
        Curve::Line {
            start: at(39.4, 8.8),
            end: at(20.0, 8.8),
        },
        Curve::Line {
            start: at(20.0, 8.8),
            end: at(20.0, 4.0),
        },
        Curve::Line {
            start: at(20.0, 4.0),
            end: at(41.0, 4.0),
        },
        Curve::Arc {
            center: at(33.0, 4.0),
            axis: [radial[1], -radial[0], 0.0],
            reference: radial,
            radius: 8.0,
            start: 0.0,
            end: 0.6_f64.asin(),
        },
    ];
    // Independent torus/plane/canal flux integration with div(x+10,0,0)=1.
    // The ball centres are (-10+r, sqrt((33+(8-r)cos(t))^2-(-10+r)^2),
    // 4+(8-r)sin(t)); the canal is trimmed on z=4 and z=8.8. Integrating
    // its circular sections analytically and the remaining t integral numerically
    // gives these values, stable to 1e-10 mm^3 from 8192 to 32768 panels.
    for (radius, expected) in [(1.0, 2009.800477555554), (0.5, 2010.294917895084)] {
        let ring = revolve(&profile, [0.0; 3], [0.0, 0.0, -1.0], 2.0 * PI / 3.0, true).unwrap();
        let clip = make_box([-10.0, 0.0, -10.0], [20.0, 50.0, 30.0]).unwrap();
        let body = boolean(ring, clip, BooleanOp::Intersect).unwrap().unwrap();
        let rho = 33.0 + (64.0_f64 - 2.5 * 2.5).sqrt();
        // This point is on the exact torus/plane intersection. The boolean's edge
        // is its fitted intersection curve, so select it by distance rather than
        // requiring the point to lie on that approximation to 1e-9 mm.
        let point = [-10.0, (rho * rho - 100.0).sqrt(), 6.5];
        let selected = nearest_edge(&body, point);
        blend(
            &body,
            Blend::Round,
            &[selected],
            &[radius],
            &[],
            Corners::Filled,
        )
        .unwrap();
        // Reopen the saved definition, rather than measuring only the live result.
        let (record, _) = body.to_record().unwrap();
        let (reopened, _) = Body::from_record(&record, true).unwrap();
        assert!(reopened.is_manifold_solid().unwrap());
        let report = reopened.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r{radius}: {}", report.text);
        let actual = volume(&reopened);
        assert!(
            (actual - expected).abs() < 1e-4,
            "r{radius}: {actual} vs {expected}"
        );
    }
}

/// A closed section in the plane x = `x` about (0, 20): 12 cubic Bezier pieces through the
/// points of the circle of radius `r` at every 30 degrees from `phase`. The end tangents
/// are the circle's, a third of the piece's chord long, times `1 + skew` after every third
/// joint from the first and `1 - skew` before it, and the reverse at the joint after: two
/// pieces join with one tangent direction, at speeds `skew` apart.
fn bezier_circle(x: f64, r: f64, phase: f64, skew: f64) -> Vec<Curve> {
    let at = |i: u32| {
        let t = phase + std::f64::consts::TAU * f64::from(i % 12) / 12.0;
        let sign = match i % 3 {
            1 => 1.0,
            2 => -1.0,
            _ => 0.0,
        };
        (
            Vec3::new(x, r * t.sin(), 20.0 - r * t.cos()),
            Vec3::new(0.0, t.cos(), t.sin()),
            skew * sign,
        )
    };
    (0..12)
        .map(|i| {
            let ((p0, t0, k0), (p3, t3, k3)) = (at(i), at(i + 1));
            let third = p0.distance(p3) / 3.0;
            let p1 = p0 + t0 * (third * (1.0 + k0));
            let p2 = p3 - t3 * (third * (1.0 - k3));
            Curve::Nurbs {
                degree: 3,
                points: [p0, p1, p2, p3].map(|p| [p.x, p.y, p.z, 1.0]).to_vec(),
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect()
}

/// A round on the end rim of a ruled loft whose one side face is G1 only to about 1e-10
/// rad at its knot lines (tangency-labs/solidcraft#15858). The loft runs from a circle of
/// radius 15 to one of radius 20 turned 0.01 rad, each 12 Bezier pieces that join with one
/// tangent direction. The first circle's pieces join at equal speeds and the second's at
/// speeds 1e-7 apart, and the two sections' tangents at a joint are 0.01 rad apart, so the
/// ruled face's derivative along the section, `(1 - u) a0 T0 + u a1 T1` before a joint and
/// `(1 - u) b0 T0 + u b1 T1` after it, turns the face's normal there. The round's stations
/// on those lines were solved on the pieces after and before them, a jump apart, and the
/// round did not close on itself (`NotSolid`; with the issue's own sections, where the jump
/// is 4e-10 rad, a station's feet did not converge).
///
/// The round must build. What it removes is the cone's round: the section between the end
/// plane and a wall 85.236 degrees from it (the wall leans atan(5 / 60)),
/// `r^2 (cot(alpha / 2) - (pi - alpha) / 2)` = 1.0393 for r = 2, swept round at its
/// centroid's radius, 19.46: 127.1.
#[test]
fn a_round_runs_over_knot_lines_where_a_loft_face_s_normal_turns() {
    all_fixes();
    let sections = [
        bezier_circle(-30.0, 15.0, 0.0, 0.0),
        bezier_circle(30.0, 20.0, 0.01, 1.0e-7),
    ];
    let b = solidcore_ops::sweep::loft(&sections, 1, true).unwrap();
    let before = volume(&b);
    let rim = edge_at(
        &b,
        [30.0, 20.0 * f64::sin(0.01), 20.0 - 20.0 * f64::cos(0.01)],
    );
    let made = blend(&b, Blend::Round, &[rim], &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(made.dropped, vec![false]);
    let removed = before - volume(&b);
    assert!((removed - 127.1).abs() < 1.0, "removed {removed}");
}

/// The plate of [`shallow_corner_plate`] drafted `draft` degrees in (its walls meet the cap at
/// z = 0, its outline, at 90 - `draft` degrees), with the record indices of the two edges of
/// that cap at the kink.
fn drafted_corner_plate(kink: f64, draft: f64) -> (Body, [usize; 2]) {
    let t = kink.to_radians();
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
        draft.to_radians(),
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let edges = [
        edge_at(&b, [25.0, 0.0, 0.0]),
        edge_at(&b, [50.0 + 20.0 * u[0], 20.0 * u[1], 0.0]),
    ];
    (b, edges)
}

/// #16627: chamfers joined at a near-straight corner leave the shared section `2 o tan(a / 2)`
/// off, `o` their offset from the side faces and `a` the angle between those faces (the corner
/// of each chamfer's offset planes lies on its side face's offset at `o`; the shared one where
/// the two offsets cross), not their setbacks. A chamfer of setbacks `d` on one face and `d'`
/// on the other of a wedge of angle `w` stands `(d' - d cos w) / sin w` off the first: on a 120
/// degree wedge setbacks 1 stand sqrt 3 off. The Join is bounded by the largest offset, so
/// chamfers of setbacks 1, 1 x 0.5 and 0.5 x 1 on wedges of 60 and 120 degrees (walls drafted
/// 30 degrees either way) pass the check at 0.97, 1.03 and 1.5 of the kink where that offset
/// reaches the intersection tolerance 2e-5. Bounded by the setbacks, the setbacks 1 on the 120
/// degree wedge would be joined at 1.5 (`a` 1.73e-5 rad), leaving the vertex sqrt 3 * a =
/// 3.0e-5 off: forced to the Join at 2e-3 degrees, the check finds it 5.2359878e-5 off,
/// sqrt 3 * a to eight digits.
#[test]
fn chamfers_join_at_a_near_straight_corner_within_their_offsets() {
    all_fixes();
    for draft in [30.0_f64, -30.0] {
        // the walls meet the cap at w: cos w = -n_cap . n_wall
        let w = (90.0 - draft).to_radians();
        let (cw, sw) = (w.cos(), w.sin());
        let d = draft.to_radians();
        for (top, side) in [(1.0, 1.0), (1.0, 0.5), (0.5, 1.0)] {
            let offsets = [(side - cw * top) / sw, (top - cw * side) / sw];
            let size = offsets[0].max(offsets[1]);
            let a_max = 2.0 * (2e-5 / (2.0 * size)).atan();
            let k_max = ((a_max.cos() - d.sin().powi(2)) / d.cos().powi(2))
                .acos()
                .to_degrees();
            for f in [0.97, 1.03, 1.5] {
                let kink = f * k_max;
                let (b, edges) = drafted_corner_plate(kink, draft);
                let what = format!("chamfer {top} x {side}, draft {draft}, {kink} degrees");
                if top == side {
                    blend(&b, Blend::Chamfer, &edges, &[top], &[], Corners::Filled)
                        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
                } else {
                    let along: Vec<usize> = edges
                        .iter()
                        .map(|&e| {
                            let faces = edge_faces_along(&b, e, [0.0, 0.0, -1.0]).unwrap();
                            if faces[0].1 > faces[1].1 {
                                faces[0].0
                            } else {
                                faces[1].0
                            }
                        })
                        .collect();
                    chamfer_sides(&b, &edges, &along, top, ChamferSecond::Distance(side))
                        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
                }
                let report = b.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "{what}: {report}");
            }
        }
    }
}

/// #13911: the inner top outline of the issue's phone case, as SolidCraft passes it to the
/// chamfer: lines, splines and arcs of radius 1.6 only 0.013 long, 17 edges in one closed loop
/// on the plane z = 9.41 between vertical walls, the case's material outside the loop. At the 8
/// arc-to-spline vertices the walls meet 3.4e-3 degrees off straight. The record is the body
/// SolidCraft's trace gives the blend (`tests/data/issue_13911_inner_top_outline.bodies.json.gz`), and the edges are its picks, in
/// its order (record indices, through the record's order).
const ISSUE_13911_EDGES: [usize; 17] = [
    170, 169, 171, 168, 172, 167, 173, 166, 174, 165, 175, 164, 176, 163, 177, 162, 178,
];

/// The issue's body and its record order.
fn issue_13911_body() -> (Body, Order) {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/data/issue_13911_inner_top_outline.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    Body::from_record(&record, true).unwrap()
}

/// The length of edge `e` of `brep`: `|r'|` integrated by 8-point Gauss-Legendre on 256 spans
/// (exact to rounding for lines and arcs, and far below the tests' tolerances for the cubic
/// splines and the rails). Kept independent of the kernel's own edge length on purpose: it
/// is the test's oracle, a cross-check on the kernel rather than a call into it.
fn edge_length(k: &Kernel, brep: &solidcore::Brep, e: EdgeId) -> f64 {
    const X: [f64; 4] = [
        0.183_434_642_495_649_8,
        0.525_532_409_916_329,
        0.796_666_477_413_626_7,
        0.960_289_856_497_536_3,
    ];
    const W: [f64; 4] = [
        0.362_683_783_378_362,
        0.313_706_645_877_887_3,
        0.222_381_034_453_374_5,
        0.101_228_536_290_376_3,
    ];
    let c = edge::curve(brep, e).unwrap().unwrap().clone();
    let ivl = edge::interval(brep, e).unwrap();
    let (t0, t1) = (ivl.min(), ivl.max());
    let n = 256;
    let h = (t1 - t0) / f64::from(n);
    let mut length = 0.0;
    for j in 0..n {
        let mid = t0 + (f64::from(j) + 0.5) * h;
        for (x, w) in X.iter().zip(W) {
            for t in [mid - x * h / 2.0, mid + x * h / 2.0] {
                let d = c.evaluate(k, t, 1, false, true).unwrap();
                length += w * h / 2.0 * d[1].length();
            }
        }
    }
    length
}

/// The loop's length and the angles its edges meet at in the plane (the kinks), from the
/// input body.
fn issue_13911_loop(b: &Body, edges: &[usize]) -> (f64, Vec<f64>) {
    let (k, brep) = kernel_copy(b);
    let mut length = 0.0;
    // each end: its point and the edge's unit direction leaving it
    let mut ends: Vec<(Vec3, Vec3)> = Vec::new();
    for &i in edges {
        let e = brep.edge_list()[i];
        length += edge_length(&k, &brep, e);
        let c = edge::curve(&brep, e).unwrap().unwrap().clone();
        let ivl = edge::interval(&brep, e).unwrap();
        for (t, sign, left) in [(ivl.min(), 1.0, false), (ivl.max(), -1.0, true)] {
            let d = c.evaluate(&k, t, 1, left, true).unwrap();
            ends.push((d[0], d[1] * (sign / d[1].length())));
        }
    }
    // the kink at each vertex: pi less the angle between the two edges leaving it
    let mut kinks = Vec::new();
    for (i, &(p, t)) in ends.iter().enumerate() {
        for &(q, u) in &ends[i + 1..] {
            if p.distance(q) < 1e-4 {
                kinks.push(t.cross(u).length().atan2(-t.dot(u)));
            }
        }
    }
    (length, kinks)
}

/// The lengths of the blend's two rails round the loop, on the top (z = 9.41) and on the walls
/// (`depth` below it): the result's edges whose middles stand `reach` off the input's loop.
fn issue_13911_rails(
    input: &Body,
    loop_edges: &[usize],
    made: &Body,
    reach: f64,
    depth: f64,
) -> [f64; 2] {
    let (k0, b0) = kernel_copy(input);
    let (k, b) = kernel_copy(made);
    let mut rails = [0.0; 2];
    for &e in b.edge_list() {
        let c = edge::curve(&b, e).unwrap().unwrap().clone();
        let ivl = edge::interval(&b, e).unwrap();
        let mid = c.evaluate(&k, ivl.evaluate(0.5), 0, false, true).unwrap()[0];
        let off = loop_edges
            .iter()
            .map(|&i| {
                edge_closest_point(&k0, &b0, b0.edge_list()[i], mid)
                    .unwrap()
                    .closest
                    .distance
            })
            .fold(f64::INFINITY, f64::min);
        if (off - reach).abs() > 1e-4 {
            continue;
        }
        if (mid.z - 9.41).abs() < 1e-7 {
            rails[0] += edge_length(&k, &b, e);
        } else if (mid.z - (9.41 - depth)).abs() < 1e-7 {
            rails[1] += edge_length(&k, &b, e);
        }
    }
    rails
}

/// #13911: chamfers and rounds of the inner top outline of the issue's phone case build at
/// every size from 0.05 to 1, and each result passes `CheckLevel::All` but for the one failure
/// the input body already has (a cylinder's generator-line scale off by 4.1e-11 against
/// 2e-12, untouched by the blend). From 0.34 they failed with status 1001: the near-straight
/// arc-to-spline corners were mitred (#16627's bound), and two rails crossing 3.4e-3 degrees
/// apart do not meet. There the Join's section lies over the second blend, 1.8e-9 times the
/// size off it (`blend_corner::join_gap`), so the corners join.
///
/// The rails are the loop's offsets, closed forms of a closed planar loop with the material
/// outside it: on the plane the top rail is `L + 2 pi d` long (`L` the loop's length, `2 pi`
/// its total turning, `d` the section's reach along the plane: a chamfer's setback, a round's
/// radius on these upright walls), and on the walls, `d` down, the wall rail is `L` long. The
/// kinks put the top rail off by at most `d` times their angles, and each of the 17 rail pieces
/// may end within twice the B-rep's intersection tolerance of an exact offset's end (that
/// slack is ARGUED, not derived; so is the rails' selection, the result's edges whose middles
/// stand `reach` off the loop at the two heights).
#[test]
fn the_inner_top_outline_of_13911_chamfers_and_rounds_at_every_size() {
    let mut fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    // SolidCraft's fix set when it traced the case (the corpus list lags it by these)
    for f in [
        "brep_ray_classifies_current_trims",
        "closed_smooth_pole_tangent_limit",
        "face_swap_uv_transposes_trims",
        "fillet_chamfer_corner",
    ] {
        if !fixes.iter().any(|x| x == f) {
            fixes.push(f.to_owned());
        }
    }
    solidcore_ops::session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let (input, _) = issue_13911_body();
    let before = input.check(CheckLevel::All).unwrap();
    let known = before.to_string();
    assert!(
        !before.is_valid()
            && known.contains("SmCone::Test #5")
            && known.contains("[owner SmCylinder]"),
        "{known}"
    );
    // the loop, picked natively at the issue's points (the record's edges, independently)
    let picks: Vec<usize> = [
        [34.215, 7.4e-12, 9.41],
        [34.215_026_705_948_8, 56.426_536_712_559_21, 9.41],
        [34.215_026_706_015_58, -56.426_536_712_536_844, 9.41],
        [31.057_880_880_291_407, 69.062_880_880_112_03, 9.41],
        [34.218_953_400_137_07, -57.469_543_122_343_22, 9.41],
        [18.421_536_712_748_598, 72.220_026_705_722_15, 9.41],
        [30.278_812_353_690_66, -69.759_579_427_087_83, 9.41],
        [9.86e-11, 72.219_999_999_896_37, 9.41],
        [18.421_536_712_791_8, -72.220_026_705_723_14, 9.41],
        [-18.421_536_712_551_635, 72.220_026_705_730_56, 9.41],
        [1.41e-10, -72.219_999_999_979_59, 9.41],
        [-31.057_880_880_071_046, 69.062_880_880_150_05, 9.41],
        [-18.421_536_712_469_653, -72.220_026_705_895_69, 9.41],
        [-34.215_026_705_628_176, 56.426_536_712_606_68, 9.41],
        [-31.057_880_880_000_702, -69.062_880_880_201_66, 9.41],
        [-34.214_999_999_792_27, -1.66e-11, 9.41],
        [-34.215_026_705_616_495, -56.426_536_712_632_67, 9.41],
    ]
    .iter()
    .map(|&p| nearest_edge(&input, p))
    .collect();
    let (length, kinks) = issue_13911_loop(&input, &picks);
    assert_eq!(kinks.len(), 17, "{kinks:?}");
    let turned: f64 = kinks.iter().map(|a| a.abs()).sum();
    assert!(turned < 1e-3, "{kinks:?}");
    let xsect = {
        let (k, brep) = kernel_copy(&input);
        solidcore::topology::vertex::corner::xsect_tol_3d_of_brep(&k, &brep)
    };
    assert_eq!(xsect, 2e-5);
    for kind in [Blend::Chamfer, Blend::Round] {
        for size in [0.05, 0.33, 0.34, 0.4, 0.6, 1.0] {
            let what = format!("{kind:?} {size}");
            let (b, order) = issue_13911_body();
            blend_ordered(
                &b,
                &order,
                kind,
                &ISSUE_13911_EDGES,
                &[size],
                &[],
                Corners::Filled,
                BlendOptions::default(),
            )
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert_eq!(report.to_string(), known, "{what}");
            // on the vertical walls under the plane, the section reaches `size` along each face
            let [top, wall] = issue_13911_rails(&input, &picks, &b, size, size);
            let slack = 2.0 * 17.0 * xsect;
            let want_top = length + 2.0 * PI * size;
            assert!(
                (top - want_top).abs() <= size * turned + slack,
                "{what}: top rail {top}, closed form {want_top}"
            );
            assert!(
                (wall - length).abs() <= slack,
                "{what}: wall rail {wall}, closed form {length}"
            );
        }
    }
}

/// One closed section of [`a_rim_round_across_a_loft_s_creases_is_certified_or_refused`]: the
/// cubic Bezier pieces through 12 points at 30 degree steps from `phase` (radians, from the
/// bottom) round an ellipse in the plane x = `x` (half-axes `a` along y and `b` along z,
/// centre (x, 0, 20)), as a fit-point spline
/// gives them, each piece on [0, 1]: the tangent at a point is the chord-length one, so two
/// pieces meet in one direction at speeds in the ratio of their chords. On a circle the chords
/// are equal and the pieces join smoothly; on an ellipse they are not, and a ruled loft to it
/// creases its side along those joints.
fn fit_point_ellipse(x: f64, a: f64, b: f64, phase: f64) -> Vec<Curve> {
    let p: Vec<[f64; 3]> = (0..12)
        .map(|i| {
            let t = phase + f64::from(i) * PI / 6.0;
            [x, a * t.sin(), 20.0 - b * t.cos()]
        })
        .collect();
    let at = |i: usize| p[i % 12];
    let chord = |i: usize| {
        let (q, r) = (at(i), at(i + 1));
        ((r[1] - q[1]).powi(2) + (r[2] - q[2]).powi(2)).sqrt()
    };
    // the tangent at point i over the chord-length parameter
    let tangent = |i: usize| {
        let (q, r) = (at(i + 11), at(i + 1));
        let h = chord(i + 11) + chord(i);
        [(r[1] - q[1]) / h, (r[2] - q[2]) / h]
    };
    (0..12)
        .map(|i| {
            let (q, r, h) = (at(i), at(i + 1), chord(i));
            let (m0, m1) = (tangent(i), tangent(i + 1));
            Curve::Nurbs {
                degree: 3,
                points: vec![
                    [x, q[1], q[2], 1.0],
                    [x, q[1] + h * m0[0] / 3.0, q[2] + h * m0[1] / 3.0, 1.0],
                    [x, r[1] - h * m1[0] / 3.0, r[2] - h * m1[1] / 3.0, 1.0],
                    [x, r[1], r[2], 1.0],
                ],
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect()
}

/// #17015: a ruled loft from a circle (radius 15, x = -30) to an ellipse (half-axes 20 and
/// 18, x = 30), both through 12 fit points, its larger rim rounded r 2. The ellipse's pieces
/// meet at unequal speeds, so the loft's side is several faces meeting in creases a few
/// ten thousandths of a radian deep, and the rounds on two of them stand about the radius
/// times that angle apart where they meet on the rim. Joining them set one round's end row to
/// the other's and kept its old bound: the fillet reported success and the body then failed
/// to read back, its blend face's definition not certifying its net (0.016 off, against a
/// bound of 3.9e-6). A round by its definition is now either proved within the tolerance of
/// its definition after the join, its bound the proved one, or the join is refused and the
/// fillet with it: the body reopens from its record, or the fillet fails.
#[test]
fn a_rim_round_across_a_loft_s_creases_is_certified_or_refused() {
    all_fixes();
    let sections = [
        fit_point_ellipse(-30.0, 15.0, 15.0, 0.0),
        fit_point_ellipse(30.0, 20.0, 18.0, 0.0),
    ];
    let b = solidcore_ops::sweep::loft(&sections, 1, true).unwrap();
    assert!(
        b.counts().unwrap()[0] > 3,
        "the loft's side is one face: no crease to round across"
    );
    let rim = edge_at(&b, [30.0, 0.0, 2.0]);
    let chain = tangent_chain(&b, None, &[rim], &[], HALF_DEGREE)
        .unwrap()
        .members;
    assert!(chain.len() > 1, "the rim is one edge: {chain:?}");
    match blend(&b, Blend::Round, &chain, &[2.0], &[], Corners::Filled) {
        Ok(_) => {
            // reopened from its record, as a design is: each blend face certified again
            let (record, _) = b.to_record().unwrap();
            let (reopened, _) = Body::from_record(&record, true)
                .unwrap_or_else(|e| panic!("the rounded loft does not reopen: {e:?}"));
            let report = reopened.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{}", report.text);
        }
        // refused by the kernel: the join's own refusal, or the corner's failure it ends in
        Err(e) => assert!(
            matches!(
                e,
                Error::Unsupported(_)
                    | Error::BlendFailed { .. }
                    | Error::Status(solidcore::error::SM_ERR)
            ),
            "{e:?}"
        ),
    }
}

/// SolidCraft #17263: a 12.2 x 2.2 bar swept along an arc with its orientation kept (the arc from
/// (0, 0, 0) to (30, 0, 0) about (15, 0, 330), dipping to z = 330 - R at its middle, R = hypot(15,
/// 330)), its lower long edge at y = 0 rounded at 0.2. The faces beside the edge are 2.2 and 12.2
/// wide; the round was refused as cutting the part in pieces, naming about 0.07, because the middle
/// of a section's arc, inside the material, was classified as lying outside the part. Past that,
/// the round (a torus about the arc's axis) took its section from the solver's first one, which
/// lies past the edge's end on the lower face's ruled extension, 5.5e-6 off the half-plane through
/// the axis: the revolution failed its own check and the result was refused as no solid. It builds.
/// The bar is 12.2 x 2.2 x 30 (each section x = const is the profile moved). The round takes off
/// its corner section A = (1 - pi/4) r^2, whose centroid lies d = r (10 - 3 pi) / (12 - 3 pi)
/// inside the lower face, swept about the arc's axis through its angle theta (Pappus), and at each
/// end the wedge between the upright end face and the radial plane through the edge's end, A 15 d /
/// 330.
#[test]
fn a_round_on_a_bar_swept_along_a_wide_arc_is_not_refused_as_cutting_the_part() {
    all_fixes();
    let (c, r, w, h) = (330.0_f64, 0.2, 12.2, 2.2);
    let big = 15.0_f64.hypot(c);
    let theta = 2.0 * (15.0 / big).asin();
    let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let profile = [
        line([0.0, 0.0, 0.0], [0.0, 0.0, h]),
        line([0.0, 0.0, h], [0.0, w, h]),
        line([0.0, w, h], [0.0, w, 0.0]),
        line([0.0, w, 0.0], [0.0, 0.0, 0.0]),
    ];
    let path = [Curve::Arc {
        center: [15.0, 0.0, c],
        axis: [0.0, -1.0, 0.0],
        reference: [-15.0 / big, 0.0, -c / big],
        radius: big,
        start: 0.0,
        end: theta,
    }];
    let (b, _, _) =
        solidcore_ops::sweep::sweep_placed(&profile, &path, SweepFrame::Translate, true).unwrap();
    assert!(rel(volume(&b), w * h * 30.0) < 1e-9, "{}", volume(&b));
    let e = edge_at(&b, [15.0, 0.0, c - big]);
    // `blend` rounds `b` in place (its result names the faces it made, not a new body)
    let made = blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    assert!(!made.made.is_empty());
    let area = (1.0 - PI / 4.0) * r * r;
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = w * h * 30.0 - area * (big - d) * theta - 2.0 * area * 15.0 * d / c;
    assert!(
        (volume(&b) - want).abs() < 1e-6,
        "{} (want {want})",
        volume(&b)
    );
}

/// #17016: the round at the foot of a cylinder boss (radius 6 about the z axis, from z = 25)
/// standing through the top of a hull lofted through six elliptic sections, each the 12 fit
/// pieces of [`fit_point_ellipse`] from 15 degrees, so no piece joint lies on the top. The
/// boss meets the hull's top face in one closed edge; the round's rails cross the joints
/// beside it, so the rollover splits the round into pieces, the first starting at the edge's
/// seam and the last ending there. The seam is both ends of the closed edge's parameter, and
/// the last piece's end belongs at the parameter's end: taken at its start, the piece was
/// traced the long way round the edge and its rail trimmed to a negative interval, failing
/// the fillet ("Unable to Process Rollover").
#[test]
fn a_round_split_by_rollover_ends_its_last_piece_at_a_closed_edge_s_seam() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::BooleanOp;
    all_fixes();
    let phase = PI / 12.0;
    let sections = [
        fit_point_ellipse(-50.0, 8.0, 5.0, phase),
        fit_point_ellipse(-30.0, 18.0, 10.0, phase),
        fit_point_ellipse(-10.0, 24.0, 14.0, phase),
        fit_point_ellipse(10.0, 24.5, 14.5, phase),
        fit_point_ellipse(30.0, 20.0, 11.0, phase),
        fit_point_ellipse(50.0, 8.5, 5.5, phase),
    ];
    let hull = solidcore_ops::sweep::loft(&sections, 3, true).unwrap();
    let boss = make_cylinder([0.0, 0.0, 25.0], 6.0, 14.0).unwrap();
    let b = boolean(hull, boss, BooleanOp::Union).unwrap().unwrap();
    // the boss's foot: the edge nearest its side at x = 6 on the hull's top
    let (k, brep) = kernel_copy(&b);
    let foot = Vec3::new(6.0, 0.0, 34.0);
    let distance = |e: EdgeId| edge_closest_point(&k, &brep, e, foot).unwrap().closest;
    let (index, near) = brep
        .edge_list()
        .iter()
        .enumerate()
        .map(|(i, &e)| (i, distance(e)))
        .min_by(|a, c| a.1.distance.total_cmp(&c.1.distance))
        .unwrap();
    assert!(
        (near.point.x.hypot(near.point.y) - 6.0).abs() < 1e-6 && near.point.z > 25.0,
        "the edge picked is not the boss's foot: {near:?}"
    );
    drop(k);
    let r = 1.0;
    let before = volume(&b);
    let [faces, _, _] = b.counts().unwrap();
    let made = blend(&b, Blend::Round, &[index], &[r], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the boss's foot does not round: {e:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    // The round adds its faces, one a piece, and cuts one face in two: the hull's top face
    // between the spline joints on either side of the foot (at |y| < 6.45 there, under the
    // round's reach of 6 + r) is crossed by the round from joint to joint on both sides of
    // the boss, leaving a front and a back face. Every other face it meets is trimmed back.
    // Its edge and vertex counts depend on how many pieces the rollover makes along the
    // joints, which the construction does not fix.
    assert!(!made.made.is_empty());
    assert_eq!(b.counts().unwrap()[0], faces + made.made.len() + 1);
    // The round fills the concave foot. Its section in a plane through the boss's axis is the
    // region between the two walls and the ball's arc: r^2 (cot(a / 2) - (pi - a) / 2) for
    // walls meeting at a (through the air). The boss is upright, and the hull's top under the
    // foot slopes less than atan(0.16) (steepest across the sections, half-axes 24 and 14 at
    // |y| = 6: dz/dy = (14 / 24^2) 6 / sqrt(1 - (6 / 24)^2) = 0.151; along x the half-axes
    // change by 0.5 over 20), so a lies in [88, 100] degrees and the section's area in
    // [0.141, 0.233] r^2. By Pappus the added volume is the integral of that area round the
    // axis at its centroid's radius, between 6 and 6 + r: in [0.141 2 pi 6, 0.233 2 pi 7] r^3
    // = [5.3, 10.3] r^3 for r = 1.
    let added = volume(&b) - before;
    assert!((5.3..=10.3).contains(&added), "{added}");
    // Each face the round made lies within the ball's reach of the foot: no farther out from
    // the boss's side than r cot(44 degrees) < 1.04 r, and above the hull's top under the
    // foot (z = 20 + 14 cos(asin(6 / 24)) = 33.55 at |y| = 6 on the lower of the two
    // sections beside it) less r, with 0.5 for the loft between them, up to the boss's top.
    let (k, brep) = kernel_copy(&b);
    for &f in &made.made {
        let face = brep.face_list()[f];
        let bx = face_bounding_box(&k, &brep, face, true).unwrap();
        for p in [bx.min(), bx.max()] {
            assert!(
                p.x.abs() <= 6.0 + 1.04 * r && p.y.abs() <= 6.0 + 1.04 * r,
                "{bx:?}"
            );
            assert!(p.z >= 33.55 - r - 0.5 && p.z <= 39.0, "{bx:?}");
        }
    }
}
