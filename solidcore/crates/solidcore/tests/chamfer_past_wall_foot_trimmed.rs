//! A chamfer whose leg runs off its face past a wall's foot into the body's material, built by
//! the cliff overflow's trim at creases (`fillet_overflow_trim_at_creases`) as the subtraction
//! already builds it. Two defects of the trim stopped it: the extension's `takes_off` probed the
//! material a hundredth of the run's length beside the run, which landed on the far side of a
//! strip narrower than that (the leg just past the foot), so the one loop that closes was refused
//! ("does not close on the face in one way"); and the extension's runs from the chain's ends to
//! its feet were not imprinted on the walls beside it, so what the blend takes off them was not
//! fenced ("reaches every face of the body").
//!
//! The body is `wr631_cliff_overflow_choice`'s stepped bar: its profile in the xy plane,
//! `(0,0) (10,0) (10,5) (7,5) (7,10) (0,10)`, swept 20 along z. The edge at (10,5) is convex
//! between the 5 high side and the 3 wide step; the step's foot (7,5) is concave, the wall
//! x = 7 rising from it. A chamfer of d > 3 on (10,5) has its leg on the step's plane at
//! x = 10 - d, past the foot, under the wall; the step's plane is carried on to it, so the
//! chamfer takes off `d^2 / 2` per length, as the subtraction path builds it. Graded on that
//! closed form swept 20 along z, a manifold solid and `check_brep` at [`CheckLevel::Level2`].

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::p3::NoRoomReason;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const PROFILE: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [10.0, 0.0],
    [10.0, 5.0],
    [7.0, 5.0],
    [7.0, 10.0],
    [0.0, 10.0],
];
const LENGTH: f64 = 20.0;
const BAR: f64 = (10.0 * 5.0 + 7.0 * 5.0) * LENGTH;

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_overflow_trim_at_creases = true;
    k
}

fn stepped_bar(k: &mut Kernel) -> Brep {
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), LENGTH, true).unwrap()
}

fn edge_at(k: &Kernel, b: &Brep, x: f64, y: f64) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let on = |p: Point3| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
            on(c.evaluate_point(k, ivl.min()).unwrap())
                && on(c.evaluate_point(k, ivl.max()).unwrap())
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
    Built { volume: f64 },
    Refused(Option<NoRoomReason>, String),
}

fn blend_step_rim(r: f64, xsect: u32) -> Outcome {
    let mut k = kernel();
    let mut b = stepped_bar(&mut k);
    let e = edge_at(&k, &b, 10.0, 5.0);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, xsect, 1, 1.0)
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
            let built = Outcome::Built {
                volume: volume(&k, &mut b),
            };
            b.use_arena_naming().unwrap();
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
            built
        }
        Err(_) => Outcome::Refused(info.no_room_reason(), info.comment),
    }
}

/// The chamfer at the same edge: its leg on the step's plane under the wall, `d^2 / 2` per
/// length taken off (as the subtraction path builds it).
#[test]
fn a_chamfer_past_the_wall_foot_is_trimmed() {
    for d in [3.2, 4.0, 4.9] {
        let want = BAR - d * d / 2.0 * LENGTH;
        match blend_step_rim(d, 0) {
            Outcome::Built { volume, .. } => assert!(
                (volume - want).abs() < 1e-6 * BAR,
                "d {d}: volume {volume}, closed form {want}"
            ),
            Outcome::Refused(why, comment) => panic!("d {d}: refused {why:?}: {comment}"),
        }
    }
}
