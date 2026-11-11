//! A round whose end runs past the far end of its side edge at an Nx1 corner, built as a cliff
//! overflow with its open end cut by the end face's plane. `EndSectionsFitTheirFaces` (the
//! fork's a307480) finds each rail's crossing of its side edge to judge whether the end section
//! runs off its end face; a rail past the side edge's far end P crosses none, so the end was left
//! unjudged and the End refused it ("the round's contact line runs past the end of its side edge
//! at the corner (onto the faces beyond)"). Now a rail whose end beside the vertex projects past P
//! on the side edge's line, where the end face has a concave edge at P, overflows its end face.
//!
//! The body is a stepped block: the profile `(0,0) (40,0) (40,10) (20,10) (20,12) (0,12)` in the
//! xz plane, swept 20 along y. Its front cap (y = 0) is one L-shaped face F. The edge from
//! (0,0,12) to (20,0,12) is the raised part's top-front edge; at (20,0,12) it ends against the
//! step's riser G (x = 20), whose side edge on F runs down to P = (20,0,10), 2 long. A round of
//! radius r > 2 has its rail on F at z = 12 - r, below P: its end section starts past P, under
//! the material of the lower part. The end is cut by the riser's plane, so the round takes off
//! its straight section over the edge's whole length: `r^2 (1 - pi/4) 20`. Graded on that closed
//! form, a manifold solid and `check_brep` at [`CheckLevel::Level2`].

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const PROFILE: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [40.0, 0.0],
    [40.0, 10.0],
    [20.0, 10.0],
    [20.0, 12.0],
    [0.0, 12.0],
];
const DEPTH: f64 = 20.0;
const EDGE: f64 = 20.0;
const BLOCK: f64 = (40.0 * 10.0 + 20.0 * 2.0) * DEPTH;

/// The cliff overflow's flags and the trim at creases, as SolidCraft runs them.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_overflow_trim_at_creases = true;
    k
}

fn stepped_block(k: &mut Kernel) -> Brep {
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], 0.0, a[1]), Vec3::new(b[0], 0.0, b[1]))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 1.0, 0.0), DEPTH, true).unwrap()
}

fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
        })
        .expect("edge")
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

enum Outcome {
    Built(f64),
    Refused(String),
}

fn round_top_front(r: f64) -> Outcome {
    let mut k = kernel();
    let mut b = stepped_block(&mut k);
    let e = edge_between(
        &k,
        &b,
        Point3::new(0.0, 0.0, 12.0),
        Point3::new(20.0, 0.0, 12.0),
    );
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(
                is_manifold_solid(&b).unwrap(),
                "r {r}: not a manifold solid"
            );
            let v = volume(&k, &mut b);
            b.use_arena_naming().unwrap();
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
            Outcome::Built(v)
        }
        Err(_) => Outcome::Refused(info.comment),
    }
}

fn straight(r: f64) -> f64 {
    r * r * (1.0 - std::f64::consts::FRAC_PI_4) * EDGE
}

/// 1.5 stays above P (the control); the rest run past it.
const RADII: [f64; 5] = [1.5, 2.5, 3.0, 4.0, 6.0];

#[test]
fn a_round_past_its_side_edge_end_is_cut_by_the_end_face() {
    for r in RADII {
        let want = BLOCK - straight(r);
        match round_top_front(r) {
            Outcome::Built(v) => assert!(
                (v - want).abs() < 1e-6 * BLOCK,
                "r {r}: volume {v}, closed form {want}"
            ),
            Outcome::Refused(why) => panic!("r {r}: refused: {why}"),
        }
    }
}

/// The same end where the rail's face does not go on past P: a 40 x 20 x 10 box with a pocket
/// 2 deep over x in [20, 40] (its wall x = 20 the end face, its side edge on the front face
/// from (20,0,10) down to P = (20,0,8)), and a radius 2 hole beside the wall, axis (22, 0), that
/// cuts the front face away below P: the rail at z = 10 - r ends outside its face, against
/// the hole. The end is cut by the wall's plane, as before, and the hole lies past it, so the
/// round takes off its straight section over the edge's 20: `r^2 (1 - pi/4) 20`.
fn pocket_and_hole(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let pocket = create_box(k, Point3::new(20.0, -1.0, 8.0), 21.0, 9.0, 3.0).unwrap();
    let block = boolean_difference(k, block, pocket).unwrap();
    let hole = create_cylinder(k, Point3::new(22.0, 0.0, -5.0), 2.0, 20.0).unwrap();
    boolean_difference(k, block, hole).unwrap()
}

fn round_pocket_rim(r: f64) -> Outcome {
    let mut k = kernel();
    let mut b = pocket_and_hole(&mut k);
    let v0 = volume(&k, &mut b);
    let e = edge_between(
        &k,
        &b,
        Point3::new(0.0, 0.0, 10.0),
        Point3::new(20.0, 0.0, 10.0),
    );
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(
                is_manifold_solid(&b).unwrap(),
                "r {r}: not a manifold solid"
            );
            let v = volume(&k, &mut b);
            b.use_arena_naming().unwrap();
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
            Outcome::Built(v0 - v)
        }
        Err(_) => Outcome::Refused(info.comment),
    }
}

#[test]
fn a_round_past_its_side_edge_end_beside_a_hole_is_cut_by_the_end_face() {
    for r in [1.5, 3.0, 4.0] {
        match round_pocket_rim(r) {
            Outcome::Built(removed) => assert!(
                (removed - straight(r)).abs() < 1e-6 * 8000.0,
                "r {r}: removed {removed}, closed form {}",
                straight(r)
            ),
            Outcome::Refused(why) => panic!("r {r}: refused: {why}"),
        }
    }
}
