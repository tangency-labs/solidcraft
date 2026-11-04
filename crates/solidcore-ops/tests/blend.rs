//! Fillets and chamfers against closed forms (the cases of SolidCraft's `core/blend_tests.rs`,
//! on edge, face and vertex indices). Their own test binary: each holds the process kernel for
//! seconds.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used
)] // test helpers

mod common;

use common::*;

use std::f64::consts::PI;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_bounding_box;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{EdgeId, FaceId, Vec3};
use solidcore_ops::{
    blend, chamfer_sides, edge_faces_along, fillet_variable, make_box, tangent_chain, Blend, Body,
    ChamferSecond, CheckLevel, Corners, Error, FitKind, SetBack, Unblendable,
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

    // a law with a corner, one zero inside, one that starts off the edge
    let b = block();
    let e = top_front(&b);
    let start = vertex_at(&b, [0.0, 0.0, 10.0]);
    let bent = fillet_variable(
        &b,
        e,
        start,
        1,
        &[0.0, 0.0, 0.5, 1.0, 1.0],
        &[1.0, 2.0, 1.0],
    );
    assert!(matches!(bent, Err(Error::Unsupported(_))), "{bent:?}");
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
