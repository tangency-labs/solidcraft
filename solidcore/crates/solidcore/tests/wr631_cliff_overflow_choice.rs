//! WR-631 (#960; the fork's tangency-labs/smlib@8ba4667, rev 68fb9162): a blend whose rail
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

fn kernel() -> Kernel {
    let mut k = Kernel::default();
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
fn blend(x: f64, y: f64, r: f64, chamfer: bool) -> (bool, Option<(i32, String)>) {
    let mut k = kernel();
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
    assert_eq!(blend(10.0, 0.0, 6.0, false), (true, None));
}

#[test]
fn a_round_into_the_material_is_refused_and_a_chamfer_overflows() {
    assert_eq!(
        blend(10.0, 5.0, 4.0, false),
        (
            false,
            Some((
                NO_ROOM,
                "Fillet Error: a round's rail leaves the face it rolls on into the body's material (beyond a wall)".into()
            ))
        )
    );
    assert_eq!(blend(10.0, 5.0, 4.0, true), (true, None));
}

#[test]
fn a_concave_overflow_is_refused() {
    assert_eq!(
        blend(7.0, 5.0, 8.0, false),
        (
            false,
            Some((
                NO_ROOM,
                "Fillet Error: a rail leaves the face it rolls on (the blend is larger than the faces allow, and adds material)".into()
            ))
        )
    );
}

/// The typed reason of the refusal of the edge at `(x, y)`.
fn reason(x: f64, y: f64, r: f64, chamfer: bool) -> Option<NoRoomReason> {
    let mut k = kernel();
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
        reason(10.0, 5.0, 4.0, false),
        Some(NoRoomReason::RoundIntoMaterial)
    );
    assert_eq!(
        reason(7.0, 5.0, 8.0, false),
        Some(NoRoomReason::ConcaveOverflow)
    );
}

#[test]
fn a_blend_that_fits_is_no_overflow() {
    let mut k = kernel();
    let mut b = stepped_bar(&mut k);
    let e = edge_at(&k, &b, 10.0, 0.0);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], 2.0, 1, 1, 1.0)
        .unwrap();
    session.do_fillet(&mut k).unwrap();
    assert!(!session.executive().cliff_overflow);
}

/// The floor rail of the radius-3 round is x=4 along the whole sweep, whereas
/// its floor support spans only 5 <= x <= 7. It has no classified exit/return:
/// an intersecting adjoining wall alone does not certify a concave crash.
#[test]
fn a_concave_round_wholly_off_its_support_is_refused() {
    let mut k = kernel();
    let mut body = wholly_off_concave_bar(&mut k);
    let edge = edge_at(&k, &body, 7.0, 5.0);
    let mut session = FilletSession::new(&mut k, &mut body).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 3.0, 1, 1, 1.0)
        .unwrap();
    assert!(session.do_fillet(&mut k).is_err());
    assert_eq!(
        session.error_info().no_room_reason(),
        Some(NoRoomReason::ConcaveOverflow)
    );
    assert!(!session.executive().cliff_overflow);
}

/// Here the floor rail x=4 is on the floor for z in [0,5] and [15,20], and
/// crosses the finite opposing wall between them. The untrimmed r3 section adds
/// A=9*(1-pi/4). The middle section is clipped at x=5 and adds
/// B=6-9*pi/4+sqrt(2)+(9/2)*asin(1/3). The original volume is
/// 65*20+5*5*10=1550; the rounded volume is 1550+10*A+10*B.
#[test]
fn a_concave_round_crossing_a_finite_wall_is_trimmed_at_its_creases() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let mut k = kernel();
    let mut body = finite_concave_crash_bar(&mut k, false);
    let edge = edge_at(&k, &body, 7.0, 5.0);
    let mut session = FilletSession::new(&mut k, &mut body).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 3.0, 1, 1, 1.0)
        .unwrap();
    session.do_fillet(&mut k).unwrap();
    assert!(session.executive().cliff_overflow);
    drop(session);
    assert!(is_manifold_solid(&body).unwrap());
    // The topology the creases leave, every face one loop. The L prism (B y=0, R x=10,
    // T y=10, W x=7, floor y=5, left x=0, caps z=0 and z=20) plus the wall box's faces
    // above the floor (x=5, y=10, z=5, z=15); its x=0 face merges with the left face.
    // The round takes the floor's x in [4,7] where it sits on it, and in z in [5,15] the
    // floor kept only x in [5,7] beside the wall: the floor falls into two rectangles. The
    // round is one face, cut at the creases on z=5 (an arc), x=5 (a line at y=8-2*sqrt 2)
    // and z=15 (an arc). Edges per face: B, R, T, W, the two floor pieces, x=5 and y=10
    // have 4; left 8; each cap 7 (the L with its corner rounded); z=5 and z=15 have 5
    // (the floor, the arc, x=5, y=10, left); the round 8 (wall rail, two cap arcs, two
    // floor rails, two crease arcs, the crease line). 14 faces, 72 / 2 = 36 edges, and
    // by Euler V = 2 - F + E = 24 (7 per cap, the box's 4 at y=10, 3 more on each of z=5
    // and z=15).
    assert_eq!(body.faces().ids().count(), 14);
    assert_eq!(body.edges().ids().count(), 36);
    assert_eq!(body.vertices().ids().count(), 24);
    body.use_arena_naming().unwrap();
    let report = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
    assert!(report.lines().is_empty(), "{:?}", report.lines());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut body, 1e-10, &attrs).unwrap();
    let full = 9.0 * (1.0 - std::f64::consts::PI / 4.0);
    let clipped =
        6.0 - 9.0 * std::f64::consts::PI / 4.0 + 2.0_f64.sqrt() + 4.5 * (1.0_f64 / 3.0).asin();
    let expected = 1550.0 + 10.0 * full + 10.0 * clipped;
    assert!(
        (volume - expected).abs() < 1e-8 * expected,
        "{volume} vs {expected}"
    );
}

/// An L bar plus an opposite wall only over z in [5,15]. The wall overlaps
/// the floor by one unit; its foot supplies the classified exit and return.
fn finite_concave_crash_bar(k: &mut Kernel, extra_cliff: bool) -> Brep {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_box;
    let mut profile = if extra_cliff {
        vec![[0.0, 0.0], [20.0, 0.0], [20.0, 1.0], [10.0, 1.0]]
    } else {
        vec![[0.0, 0.0], [10.0, 0.0]]
    };
    profile.extend([[10.0, 10.0], [7.0, 10.0], [7.0, 5.0], [0.0, 5.0]]);
    let curves: Vec<Curve> = profile
        .iter()
        .zip(profile.iter().cycle().skip(1))
        .map(|(a, b)| {
            create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                .unwrap()
                .into()
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let base = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 20.0, true).unwrap();
    let wall = create_box(k, Point3::new(0.0, 4.0, 5.0), 5.0, 6.0, 10.0).unwrap();
    boolean_union(k, base, wall).unwrap()
}

/// The opposite wall spans the entire edge, leaving the floor rail wholly off
/// its support. No classified crossing can authorize this case.
fn wholly_off_concave_bar(k: &mut Kernel) -> Brep {
    let profile = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [7.0, 10.0],
        [7.0, 5.0],
        [5.0, 5.0],
        [5.0, 10.0],
        [0.0, 10.0],
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .zip(profile.iter().cycle().skip(1))
        .map(|(a, b)| {
            create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                .unwrap()
                .into()
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 20.0, true).unwrap()
}

#[test]
fn a_concave_crash_does_not_allow_an_unrelated_convex_overflow() {
    for reverse in [false, true] {
        let mut k = kernel();
        let mut body = finite_concave_crash_bar(&mut k, true);
        let mut edges = [edge_at(&k, &body, 7.0, 5.0), edge_at(&k, &body, 20.0, 0.0)];
        if reverse {
            edges.reverse();
        }
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, 3.0, 1, 1, 1.0)
            .unwrap();
        assert!(session.do_fillet(&mut k).is_err());
        assert_eq!(
            session.error_info().no_room_reason(),
            Some(NoRoomReason::OverflowWithConcaveBlend)
        );
        assert!(!session.executive().cliff_overflow);
    }
}
