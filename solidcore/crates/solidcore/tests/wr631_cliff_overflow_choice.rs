//! WR-631 (#960; the fork's tangency-labs/smlib@8ba4667, rev 68fb9162): with
//! `fillet_refuse_rails_leaving_faces` and `fillet_cliff_overflow`, a blend whose rail
//! leaves its face between its ends is a cliff overflow when it is convex and its rails
//! leave across cliffs, and is refused with its own `NoRoom` message otherwise.
//!
//! The body is a stepped bar, its profile in the xy plane swept 20 along z:
//! `(0,0) (10,0) (10,5) (7,5) (7,10) (0,10)`. The edge at (10,0) is convex between the
//! floor-less bottom (10 wide) and the 5 high side; the edge at (10,5) is convex between
//! that side and the 3 wide step; the edge at (7,5) is concave, the step's foot.
//!
//! - r6 at (10,0): the rail on the 5 high side runs past its rim (10,5), a cliff: an
//!   overflow, `cliff_overflow` set, and built (`wr631_cliff_overflow_construction`).
//! - r4 at (10,5): the rail on the 3 wide step runs into the wall beyond its foot: a
//!   round is refused ("into the body's material"); a chamfer is an overflow.
//! - r8 at (7,5): concave, the rail on the step runs past its rim: refused ("adds
//!   material").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::p3::NoRoomReason;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
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

const GENERIC: &str =
    "Fillet Error: a rail leaves the face it rolls on (the blend is larger than the faces allow)";

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = cliff;
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
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 20.0, true).unwrap()
}

/// The swept edge through `(x, y)`.
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

/// Rounds (`chamfer` false) or chamfers the edge at `(x, y)`: whether it was taken for a
/// cliff overflow, and the fillet error code and comment of a refusal (`None`: built).
fn blend(cliff: bool, x: f64, y: f64, r: f64, chamfer: bool) -> (bool, Option<(i32, String)>) {
    let mut k = kernel(cliff);
    let mut b = stepped_bar(&mut k);
    let e = edge_at(&k, &b, x, y);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    let xsect = u32::from(!chamfer); // 0 linear, 1 circular
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, xsect, 1, 1.0)
        .unwrap();
    let built = session.do_fillet(&mut k).is_ok();
    let info = session.error_info();
    (
        session.executive().cliff_overflow,
        (!built).then(|| (info.code, info.comment.clone())),
    )
}

const NO_ROOM: i32 = FilletErrorType::NoRoom as i32;

#[test]
fn a_convex_round_over_a_cliff_is_a_cliff_overflow() {
    assert_eq!(blend(true, 10.0, 0.0, 6.0, false), (true, None));
    // without the flag: refused as before, no overflow
    assert_eq!(
        blend(false, 10.0, 0.0, 6.0, false),
        (false, Some((NO_ROOM, GENERIC.into())))
    );
}

#[test]
fn a_round_into_the_material_is_refused_and_a_chamfer_overflows() {
    assert_eq!(
        blend(true, 10.0, 5.0, 4.0, false),
        (
            false,
            Some((
                NO_ROOM,
                "Fillet Error: a round's rail leaves the face it rolls on into the body's material (beyond a wall)".into()
            ))
        )
    );
    assert_eq!(blend(true, 10.0, 5.0, 4.0, true), (true, None));
}

#[test]
fn a_concave_overflow_is_refused() {
    assert_eq!(
        blend(true, 7.0, 5.0, 8.0, false),
        (
            false,
            Some((
                NO_ROOM,
                "Fillet Error: a rail leaves the face it rolls on (the blend is larger than the faces allow, and adds material)".into()
            ))
        )
    );
    assert_eq!(
        blend(false, 7.0, 5.0, 8.0, false).1.map(|e| e.0),
        Some(NO_ROOM)
    );
}

/// The typed reason of the refusal of the edge at `(x, y)` (`cliff`: with the cliff
/// overflow).
fn reason(cliff: bool, x: f64, y: f64, r: f64, chamfer: bool) -> Option<NoRoomReason> {
    let mut k = kernel(cliff);
    let mut b = stepped_bar(&mut k);
    let e = edge_at(&k, &b, x, y);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, u32::from(!chamfer), 1, 1.0)
        .unwrap();
    assert!(session.do_fillet(&mut k).is_err());
    session.error_info().no_room_reason()
}

#[test]
fn each_refusal_names_its_reason() {
    assert_eq!(
        reason(false, 10.0, 0.0, 6.0, false),
        Some(NoRoomReason::RailLeavesFace)
    );
    assert_eq!(
        reason(true, 10.0, 5.0, 4.0, false),
        Some(NoRoomReason::RoundIntoMaterial)
    );
    assert_eq!(
        reason(true, 7.0, 5.0, 8.0, false),
        Some(NoRoomReason::ConcaveOverflow)
    );
}

#[test]
fn a_blend_that_fits_is_no_overflow() {
    let mut k = kernel(true);
    let mut b = stepped_bar(&mut k);
    let e = edge_at(&k, &b, 10.0, 0.0);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], 2.0, 1, 1, 1.0)
        .unwrap();
    session.do_fillet(&mut k).unwrap();
    assert!(!session.executive().cliff_overflow);
}
