//! #2403 (carried over from tangency-labs/smlib#94; the generic blend, hub #2243): a tangent
//! chain run over an earlier round's end curve, where that round has the chain's radius.
//!
//! The part is SolidCraft #851's knife-edge plate (capi `knife_edge_arc_part`): a 41 1/6 x 21
//! plate and the corner a circle of radius 21 about (62 1/6, -21) leaves, 10 high. At its front
//! corner (41 1/6, -21, 10) three edges meet: the top front edge, the top edge of the concave
//! R21 arc wall and the front upright at the arc. SolidCraft's rig rounds them R2 one fillet at a
//! time, each with its tangent chain (SolidCraft's `tangent_chain` default):
//! - `851-fillet-arc-then-top-front`: the arc's top edge, then the top front edge with the arc
//!   round's end curve on the front face and the upright under it;
//! - `851-fillet-top-front-then-upright`: the top front edge, then the upright with the first
//!   round's end curve on the wall and the arc's top edge.
//!
//! Along the end curve the second round's supports are the front face (or the wall) and the
//! earlier round. That round has the ball's radius, so its offset towards the ball is its own
//! spine (the arc round's centre circle, the top front round's axis): the new spine meets it
//! in one point, and the piece along the end curve is the ball about that point, the ball the
//! three faces front, top and wall touch: `C = (62 1/6 - sqrt(23^2 - 2^2), -19, 8)`. Both
//! orders end on the shape the three edges rounded together give (the fork's capi
//! `ball_knife_edge_arc_corner`): the corner face that ball's triangle between its contacts
//! with the three faces, G1 to the three rounds, and the same volume. SolidCraft's
//! expected.json gives that volume as 9524.342896 to its grid's 5e-3 (the capi's check).
//!
//! Before the fix both orders failed with status 1001: the round along the end curve was
//! traced, and its offsets meet in one point only. The end curve where the earlier round ends
//! on a plane square to its axis (the top front round's end arc on the left face, rig case
//! `851-fillet-top-front-then-front-left-upright`) already built, as the round about that axis
//! whose tube-centre radius is 0; it is unchanged.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, BIG_DOUBLE};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::topology_traverser::face_edges;
use solidcore::topology::{edge, EdgeId, FaceId};
use solidcore::{Brep, Kernel};

const TIP: f64 = 62.0 + 1.0 / 6.0;
const WALL: f64 = 21.0;
const RAD: f64 = 2.0;

/// SolidCraft's expected.json volume of the three edges rounded R2 together, to its grid's
/// 5e-3 (the fork's capi `scenario_three_edges_at_the_knife_edge_arc_corner`).
const SOLIDCRAFT_VOLUME: f64 = 9524.342896;

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn kernel(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// capi `knife_edge_arc_part`: the plate joined to the corner box less the wall's cylinder.
fn part(k: &mut Kernel) -> Brep {
    let x0 = TIP - WALL;
    let plate = create_box(k, Point3::new(0.0, -WALL, 0.0), x0, WALL, 10.0).unwrap();
    let corner = create_box(k, Point3::new(x0, -WALL, 0.0), WALL, WALL, 10.0).unwrap();
    let disc = create_cylinder(k, Point3::new(TIP, -WALL, 0.0), WALL, 10.0).unwrap();
    let corner = boolean_difference(k, corner, disc).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.05 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap_or_else(|| panic!("no edge at {at:?}")).0
}

/// Rounds the edges at `at` R2 together (one fillet), or the executive's error.
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]]) -> Result<(), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![RAD; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err((info.code, info.comment.clone()));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

// the edges, by a point on each
const TOP_FRONT: [f64; 3] = [20.0, -21.0, 10.0];
const ARC_TOP: [f64; 3] = [47.3174, -6.1508, 10.0];
const UPRIGHT: [f64; 3] = [41.1667, -21.0, 5.0];
const LEFT_UPRIGHT: [f64; 3] = [0.0, -21.0, 5.0];
const LEFT_TOP: [f64; 3] = [0.0, -10.0, 10.0];

/// The arc round's end curve on the front face: its section in the plane y = -21 through
/// the wall's axis, the circle of radius 2 about (TIP - 23, -21, 8), at 45 degrees.
fn arc_round_end_curve() -> [f64; 3] {
    let h = RAD / 2.0f64.sqrt();
    [TIP - 23.0 + h, -21.0, 8.0 + h]
}

/// The top front round's end curve on the wall (its cylinder about y = -19, z = 8 met by the
/// wall rho = 21), at 45 degrees round its axis.
fn top_front_round_end_curve() -> [f64; 3] {
    let h = RAD / 2.0f64.sqrt();
    let (y, z) = (-19.0 - h, 8.0 + h);
    [TIP - (WALL * WALL - (y + 21.0) * (y + 21.0)).sqrt(), y, z]
}

/// The top front round's end arc on the left face x = 0, at 45 degrees.
fn top_front_round_end_arc() -> [f64; 3] {
    let h = RAD / 2.0f64.sqrt();
    [0.0, -19.0 - h, 8.0 + h]
}

/// The ball the front, the top and the wall touch (the offsets y = -19, z = 8 and rho = 23
/// about the wall's axis), and its contacts with them: the corner's three vertices.
fn arc_corner_ball() -> (Point3, [Point3; 3]) {
    let cx = TIP - (23.0f64 * 23.0 - RAD * RAD).sqrt();
    let k = WALL / 23.0;
    (
        Point3::new(cx, -19.0, 8.0),
        [
            Point3::new(cx, -19.0, 10.0),
            Point3::new(cx, -21.0, 8.0),
            Point3::new(TIP + (cx - TIP) * k, -21.0 + RAD * k, 8.0),
        ],
    )
}

/// The faces of `b` whose vertices are the three points `v` (within 1e-9).
fn corner_faces(b: &Brep, v: &[Point3; 3]) -> Vec<FaceId> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let mut pts: Vec<Point3> = Vec::new();
        for e in face_edges(b, f).unwrap() {
            let v0 = edge::vertex(b, e).unwrap();
            let v1 = edge::other_vertex(b, e, v0).unwrap();
            for vv in [v0, v1].into_iter().flatten() {
                let p = b.vertex(vv).unwrap().point;
                if !pts.iter().any(|q| q.distance(p) <= 1e-9) {
                    pts.push(p);
                }
            }
        }
        if pts.len() == 3 && v.iter().all(|w| pts.iter().any(|p| p.distance(*w) <= 1e-9)) {
            out.push(f);
        }
    }
    out
}

/// What the corner face of `b` (the one face whose vertices are `v`) is: the worst distance
/// from the sphere about `c` of radius 2 of its surface (sampled 17 x 17 over its natural
/// domain) and of its edges, and the worst sine of the angle between that sphere's normal and
/// the normal of the face across each of its edges (sampled along that face's UV trim curve):
/// G1 where it is at rounding.
fn corner_face(k: &Kernel, b: &Brep, c: Point3, v: &[Point3; 3]) -> (f64, f64, f64) {
    let faces = corner_faces(b, v);
    assert_eq!(
        faces.len(),
        1,
        "one corner face with the ball's three contacts"
    );
    let f = faces[0];
    let s = face2::surface(b, f).unwrap().unwrap();
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut surf_off = 0.0f64;
    for i in 0..=16 {
        for j in 0..=16 {
            let uv = dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0);
            let p = s.evaluate_point(k, uv).unwrap();
            surf_off = surf_off.max(((p - c).length() - RAD).abs());
        }
    }
    let (mut edge_off, mut g1_off) = (0.0f64, 0.0f64);
    for e in face_edges(b, f).unwrap() {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        for t in 0..=32 {
            let p = curve
                .evaluate_point(k, ivl.min() + (ivl.max() - ivl.min()) * f64::from(t) / 32.0)
                .unwrap();
            edge_off = edge_off.max(((p - c).length() - RAD).abs());
        }
        for eu in edge::edgeuses(b, e, None, None).unwrap() {
            let Some(other) = eu1::face(b, eu).unwrap() else {
                continue;
            };
            if other == f {
                continue;
            }
            let os = face2::surface(b, other).unwrap().unwrap();
            let uv = eu1::uv_trim_curve(b, eu).unwrap().0.unwrap().clone();
            let ui = uv.natural_interval();
            for t in 1..32 {
                let w = uv
                    .evaluate_point(k, ui.evaluate(f64::from(t) / 32.0))
                    .unwrap();
                let at = Point2::new(w.x, w.y);
                let q = os.evaluate_point(k, at).unwrap();
                let n = os
                    .evaluate_normal(k, at, Side::Right, Side::Right)
                    .unwrap()
                    .unitize()
                    .unwrap();
                let radial = (q - c).unitize().unwrap();
                g1_off = g1_off.max(n.cross(radial).length());
            }
        }
    }
    (surf_off, edge_off, g1_off)
}

/// The rounded part's soundness, face count, volume and its arc corner's face.
fn assert_arc_corner_ball(name: &str, k: &mut Kernel, b: &mut Brep) -> f64 {
    let v = volume(k, b);
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    let (c, contacts) = arc_corner_ball();
    let (surf, edges, g1) = corner_face(k, b, c, &contacts);
    let faces = get_faces(b).unwrap().len();
    eprintln!(
        "{name}: volume {v:.9} (SolidCraft {SOLIDCRAFT_VOLUME}); {faces} faces; corner face off \
         its ball {surf:.3e}, edges {edges:.3e}, G1 {g1:.3e}; check {lines:?}"
    );
    assert!(lines.is_empty(), "{name}: {lines:?}");
    assert!(
        is_manifold_solid(b).unwrap(),
        "{name}: not a manifold solid"
    );
    // the six faces of the part, the three rounds and the corner
    assert_eq!(faces, 10, "{name}");
    assert!(surf <= 1e-12 && edges <= 1e-12, "{name}: {surf} {edges}");
    assert!(g1 <= 1e-9, "{name}: {g1}");
    assert!((v - SOLIDCRAFT_VOLUME).abs() < 5e-3, "{name}: {v}");
    v
}

/// The three edges R2 together, the shape both orders end on (passes before the fix).
fn three_together() -> f64 {
    let mut k = kernel(true);
    let mut b = part(&mut k);
    round(&mut k, &mut b, &[TOP_FRONT, ARC_TOP, UPRIGHT]).unwrap();
    assert_arc_corner_ball("three together", &mut k, &mut b)
}

#[test]
fn three_edges_together_end_on_the_corner_ball() {
    three_together();
}

/// `851-fillet-arc-then-top-front`: the arc round ends on the front face in its section, a
/// circle of radius 2 in the plane y = -21; the top front edge's chain over it ends on the
/// ball about C, which lies on the arc round's centre circle: the corner face is that ball's
/// triangle, and the arc round is cut back to its section through C.
#[test]
fn arc_then_top_front_ends_on_the_corner_ball() {
    let mut k = kernel(true);
    let mut b = part(&mut k);
    round(&mut k, &mut b, &[ARC_TOP]).unwrap();
    round(&mut k, &mut b, &[TOP_FRONT, arc_round_end_curve(), UPRIGHT])
        .unwrap_or_else(|e| panic!("the top front chain failed: {e:?}"));
    let v = assert_arc_corner_ball("arc then top front", &mut k, &mut b);
    let want = three_together();
    assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
}

/// `851-fillet-top-front-then-upright`: the top front round ends on the wall in a space
/// curve; the upright's chain over it ends on the ball about C, which lies on the top front
/// round's axis.
#[test]
fn top_front_then_upright_ends_on_the_corner_ball() {
    let mut k = kernel(true);
    let mut b = part(&mut k);
    round(&mut k, &mut b, &[TOP_FRONT]).unwrap();
    round(
        &mut k,
        &mut b,
        &[UPRIGHT, top_front_round_end_curve(), ARC_TOP],
    )
    .unwrap_or_else(|e| panic!("the upright chain failed: {e:?}"));
    let v = assert_arc_corner_ball("top front then upright", &mut k, &mut b);
    let want = three_together();
    assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
}

/// Without `fillet_blend_corner` the round along the end curve is traced, as in the fork,
/// and fails.
#[test]
fn without_the_blend_corner_the_chain_over_the_end_curve_fails() {
    for arc_first in [true, false] {
        let mut k = kernel(false);
        let mut b = part(&mut k);
        let r = if arc_first {
            round(&mut k, &mut b, &[ARC_TOP]).unwrap();
            round(&mut k, &mut b, &[TOP_FRONT, arc_round_end_curve(), UPRIGHT])
        } else {
            round(&mut k, &mut b, &[TOP_FRONT]).unwrap();
            round(
                &mut k,
                &mut b,
                &[UPRIGHT, top_front_round_end_curve(), ARC_TOP],
            )
        };
        assert!(r.is_err(), "arc first {arc_first}: {r:?}");
    }
}

/// `851-fillet-top-front-then-front-left-upright`, the end arc on a plane square to the
/// round's axis: the chain over it ends on the ball about (2, -19, 8) at the left front top
/// corner, built (before and after the fix) as the round about the top front round's axis
/// whose tube-centre radius is 0. Beyond the top front round alone, it takes the upright's
/// and the left top edge's spandrels, 8 and 19 long, and at the corner the cube of side 2
/// less the ball's octant, less the top front round's spandrel along its 2:
/// `27 (4 - pi) + 8 (1 - pi/6) - 2 (4 - pi) = 27 (4 - pi) + 2 pi / 3`.
#[test]
fn the_flat_end_arc_still_ends_on_its_corner_ball() {
    let mut k = kernel(true);
    let mut b = part(&mut k);
    round(&mut k, &mut b, &[TOP_FRONT]).unwrap();
    let first = volume(&k, &mut b);
    round(
        &mut k,
        &mut b,
        &[LEFT_UPRIGHT, top_front_round_end_arc(), LEFT_TOP],
    )
    .unwrap();
    let v = volume(&k, &mut b);
    let want = first - 27.0 * (4.0 - PI) - 2.0 * PI / 3.0;
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let c = Point3::new(2.0, -19.0, 8.0);
    let contacts = [
        Point3::new(0.0, -19.0, 8.0),
        Point3::new(2.0, -21.0, 8.0),
        Point3::new(2.0, -19.0, 10.0),
    ];
    let (surf, edges, g1) = corner_face(&k, &b, c, &contacts);
    eprintln!(
        "flat end arc: volume {v:.9} want {want:.9}; corner face off its ball {surf:.3e}, \
         edges {edges:.3e}, G1 {g1:.3e}; check {lines:?}"
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(get_faces(&b).unwrap().len(), 10);
    assert!(surf <= 1e-12 && edges <= 1e-12 && g1 <= 1e-9);
    assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
}
