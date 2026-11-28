//! Rounds that would open into a hollow behind them (SolidCraft's `rounds_into_hollow` cases:
//! #1668's thin wall, #2852's U shell), run through [`blend`] with every fix on: the kernel's
//! `NoRoomReason::BreaksThrough` refuses them as [`Error::TooLarge`] with
//! [`RoomReason::BreaksThrough`].
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers
use solidcore::math::det;

mod common;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::Vec3;

use common::*;
use solidcore_ops::*;

/// The box `[lo, hi]` less the box `[cut_lo, cut_hi]`.
fn box_less_box(lo: [f64; 3], hi: [f64; 3], cut_lo: [f64; 3], cut_hi: [f64; 3]) -> Body {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    built(|k| {
        let outer = create_box(k, v(lo), hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])?;
        let inner = create_box(
            k,
            v(cut_lo),
            cut_hi[0] - cut_lo[0],
            cut_hi[1] - cut_lo[1],
            cut_hi[2] - cut_lo[2],
        )?;
        boolean_difference(k, outer, inner)
    })
}

/// #1668: a 104 x 64 x 42 box hollowed 2 mm from the inside, open at the bottom.
fn thin_walled() -> Body {
    box_less_box(
        [-52.0, -32.0, 0.0],
        [52.0, 32.0, 42.0],
        [-50.0, -30.0, -1.0],
        [50.0, 30.0, 40.0],
    )
}

/// #2852: a 3 mm upside-down U, 60 x 20 x 50, its inside top edges rounded 15.
fn u_shell() -> Body {
    let b = box_less_box(
        [0.0, 0.0, 0.0],
        [60.0, 20.0, 50.0],
        [3.0, -1.0, -1.0],
        [57.0, 21.0, 47.0],
    );
    let inside = [
        edge_at(&b, [3.0, 10.0, 47.0]),
        edge_at(&b, [57.0, 10.0, 47.0]),
    ];
    blend(&b, Blend::Round, &inside, &[15.0], &[], Corners::Filled).unwrap();
    b
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

const QUARTER: f64 = 1.0 - std::f64::consts::FRAC_PI_4;

/// The round of `edges` at `sizes` on a copy of `b`: the copy's volume, or the refusal.
fn round(b: &Body, edges: &[usize], sizes: &[f64]) -> Result<f64> {
    let c = b.copy().unwrap();
    blend(&c, Blend::Round, edges, sizes, &[], Corners::Filled).map(|_| volume(&c))
}

/// A refusal for breaking through, its fit within 0.011 below `limit` (the kernel's fit
/// margin, 0.01, under its bound), at the middle of the edge `mid`.
fn breaks_through(r: Result<f64>, limit: f64, mid: [f64; 3]) {
    match r {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            at: Some(at),
            why: Some(RoomReason::BreaksThrough),
            ..
        }) => {
            assert!(size < limit && limit - size < 0.011, "{size} vs {limit}");
            assert!(
                at.iter().zip(mid).all(|(a, m)| (a - m).abs() < 1e-9),
                "{at:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

fn close(v: f64, want: f64) {
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
}

/// #1668: the outer top edge's round, centre (32 - r, 42 - r) in (y, z), uncovers the hollow's
/// corner (30, 40) at sqrt2 (r - 2) = r, r = 4 + 2 sqrt2: R7 is refused for breaking through,
/// naming a radius just under it; R5 and R6.8 build, taking (1 - pi/4) r^2 along 104 off
/// 104 64 42 - 100 60 40 = 39552. With the inside edge rounded R5 too, which adds
/// (1 - pi/4) 25 along 100.
#[test]
fn a_round_through_a_thin_wall_is_refused_as_breaking_through() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let limit = 4.0 + 2.0 * std::f64::consts::SQRT_2;
    breaks_through(round(&wall, &[top], &[7.0]), limit, [0.0, 32.0, 42.0]);
    for r in [5.0, 6.8] {
        close(
            round(&wall, &[top], &[r]).unwrap(),
            39552.0 - QUARTER * r * r * 104.0,
        );
    }
    close(
        round(&wall, &[top, inside], &[5.0]).unwrap(),
        39552.0 - QUARTER * 25.0 * 4.0,
    );
}

/// #2852: a 3 mm U whose inside top edges are rounded R15 (centres (18, 32), (42, 32) in
/// (x, z)): its outside rounds' arcs reach the inside rounds' at R = (18 sqrt2 - 15) /
/// (sqrt2 - 1) = 25.2426. R30 is refused for breaking through, naming a radius just under
/// it; R18 (concentric) and R25 build, each taking (1 - pi/4) R^2 along 20 at both edges
/// off 60 20 50 - 54 20 47 + 2 (1 - pi/4) 225 20.
#[test]
fn a_round_past_an_inside_round_is_refused_as_breaking_through() {
    all_fixes();
    let u = u_shell();
    let base = 60.0 * 20.0 * 50.0 - 54.0 * 20.0 * 47.0 + 2.0 * QUARTER * 225.0 * 20.0;
    close(volume(&u), base);
    let outer = [
        edge_at(&u, [0.0, 10.0, 50.0]),
        edge_at(&u, [60.0, 10.0, 50.0]),
    ];
    let s2 = std::f64::consts::SQRT_2;
    let limit = (18.0 * s2 - 15.0) / (s2 - 1.0);
    match round(&u, &outer, &[30.0]) {
        Err(Error::TooLarge {
            size,
            why: Some(RoomReason::BreaksThrough),
            ..
        }) => assert!(size < limit && limit - size < 0.011, "{size} vs {limit}"),
        other => panic!("{other:?}"),
    }
    for r in [18.0, 25.0] {
        close(
            round(&u, &outer, &[r]).unwrap(),
            base - 2.0 * QUARTER * r * r * 20.0,
        );
    }
}

/// The inside edge's concave round, made in the same fillet, fills the hollow's corner the
/// outer round runs over, so `BreaksThrough` counts it as material and both build (the rig's
/// 1668-fillet-both-sides-of-thin-wall-r7). Closed form: the outer round takes the corner
/// square less the quarter disc, (1 - pi/4) 49, along 104, and the inside round adds it
/// along 100, all of what the outer one takes inside the hollow lying in what the inside one
/// adds: 39552 - 4 (1 - pi/4) 49.
#[test]
fn an_inside_round_holds_the_wall_an_outer_round_runs_over() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    for (sizes, want) in [
        (&[7.0, 7.0], 39_509.938_040_025_9),
        // the outer arc's closest approach to the inside round's centre (23, 33) is
        // 13.8 - 4.8 sqrt2 = 7.0118: 0.012 of wall between the two rounds, and the same
        // closed form, 39552 - (1 - pi/4) (13.8^2 104 - 49 100)
        (&[13.8, 7.0], 36_353.196_528_043),
    ] {
        let c = wall.copy().unwrap();
        blend(
            &c,
            Blend::Round,
            &[top, inside],
            sizes,
            &[],
            Corners::Filled,
        )
        .unwrap();
        close(volume(&c), want);
        assert!(c.check(CheckLevel::All).unwrap().is_valid(), "{sizes:?}");
    }
}

/// A concave round of the same fillet too small to fill what the outer round takes inside
/// the hollow is no fill: R8 outside with R1 inside is still refused for breaking through.
/// The outer arc (centre (24, 34)) runs through the hollow at 41.4 to 48.6 degrees; at 45
/// it is (29.657, 39.657), 0.929 from the inside round's centre (29, 39), inside its ball
/// and so not in what that round adds. The fit is where the outer arc's closest approach
/// to (29, 39), r - sqrt2 (r - 3), is 1: r = (3 sqrt2 - 1) / (sqrt2 - 1) = 7.828, above the
/// 6.828 of the outer round alone (the fit counts the fill).
#[test]
fn an_inside_round_too_small_to_fill_the_hollow_is_still_refused() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let s2 = std::f64::consts::SQRT_2;
    breaks_through(
        round(&wall, &[top, inside], &[8.0, 1.0]),
        (3.0 * s2 - 1.0) / (s2 - 1.0),
        [0.0, 32.0, 42.0],
    );
}

/// The inside round counts as a fill only where it ends alone: with the inside roof edge
/// at the end (x = 50) rounded in the same fillet, a corner patch replaces the inside round
/// at their shared vertex, so the R7 outer round is judged against the part alone and
/// refused with the fit of the outer round alone, 4 + 2 sqrt2.
#[test]
fn an_inside_round_meeting_another_round_is_no_fill() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let end = edge_at(&wall, [50.0, 0.0, 40.0]);
    breaks_through(
        round(&wall, &[top, inside, end], &[7.0]),
        4.0 + 2.0 * std::f64::consts::SQRT_2,
        [0.0, 32.0, 42.0],
    );
}

/// #1668's box with its hollow stepped short of the end: [-50, 48.6] as before, and
/// [48.6, 50] with the +y wall 1.5 thick (its inside face at y = 30.5).
fn stepped_hollow() -> Body {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    built(|k| {
        let outer = create_box(k, v([-52.0, -32.0, 0.0]), 104.0, 64.0, 42.0)?;
        let a = create_box(k, v([-50.0, -30.0, -1.0]), 98.6, 60.0, 41.0)?;
        let b = create_box(k, v([48.6, -30.0, -1.0]), 1.4, 60.5, 41.0)?;
        let part = boolean_difference(k, outer, a)?;
        boolean_difference(k, part, b)
    })
}

/// The inside round counts as a fill only where both ends of its edge are closed square. With
/// the hollow stepped short of its end (`stepped_hollow`), the inside corner edge (30, 40)
/// stops at x = 48.6, on a step face 0.5 wide that does not close the round's corner, and
/// the hollow runs on past it to x = 50 with its corner at (30.5, 40). There, between the
/// 32 steps (47.125 and 50.375), the outer round's arc (centre (32 - r, 42 - r)) uncovers
/// that corner at (r - 1.5)^2 + (r - 2)^2 = r^2: r = 3.5 + sqrt6 = 5.949. The outer round
/// is judged in every stretch where the section meets the part alike, so R7 (with the
/// inside round R7, or alone) is refused with that fit, not the 6.828 of the steps only.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_hollow_running_on_past_the_inside_round_is_refused() {
    all_fixes();
    let wall = stepped_hollow();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let limit = 3.5 + 6.0_f64.sqrt();
    breaks_through(
        round(&wall, &[top, inside], &[7.0]),
        limit,
        [0.0, 32.0, 42.0],
    );
    breaks_through(round(&wall, &[top], &[7.0]), limit, [0.0, 32.0, 42.0]);
}

/// `stepped_hollow` with a blind hole r1 drilled up into its roof from the hollow, at
/// (0, 20), z 39 to 40.5: a cylinder face 5.5 from the outer round's arc, which changes no
/// section's meets.
fn stepped_hollow_with_a_blind_hole() -> Body {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    built(|k| {
        let outer = create_box(k, v([-52.0, -32.0, 0.0]), 104.0, 64.0, 42.0)?;
        let a = create_box(k, v([-50.0, -30.0, -1.0]), 98.6, 60.0, 41.0)?;
        let b = create_box(k, v([48.6, -30.0, -1.0]), 1.4, 60.5, 41.0)?;
        let hole = create_cylinder(k, v([0.0, 20.0, 39.0]), 1.0, 1.5)?;
        let part = boolean_difference(k, outer, a)?;
        let part = boolean_difference(k, part, b)?;
        boolean_difference(k, part, hole)
    })
}

/// A curved face near the band leaves its events unknown in closed form only where it
/// meets the band itself. On `stepped_hollow` the outer round R6.5 alone keeps hollow A's
/// corner (30, 40) inside its ball (6.364 from the centre (25.5, 35.5)) but uncovers hollow
/// B's (30.5, 40) (6.727), between the 32 steps; a blind hole in the roof 5.5 from the arc
/// does not change that: refused with the fit against B's corner, 3.5 + sqrt6.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_curved_face_clear_of_the_band_leaves_its_stretches_judged() {
    all_fixes();
    let limit = 3.5 + 6.0_f64.sqrt();
    for wall in [stepped_hollow(), stepped_hollow_with_a_blind_hole()] {
        let top = edge_at(&wall, [0.0, 32.0, 42.0]);
        breaks_through(round(&wall, &[top], &[6.5]), limit, [0.0, 32.0, 42.0]);
    }
}

/// #1668's box with its hollow turned 0.02 rad (as its tangent) about z: the inside corner
/// edge runs skew to the outer top edge, the wall thinning from 3 to 1 along it, and the
/// hollow's ends stay square to that edge.
#[allow(clippy::disallowed_methods)] // the test's own placement, not ops computing geometry
fn tapered_hollow() -> Body {
    use solidcore_ops::boolean::boolean;
    let hollow = make_box([-50.0, -30.0, -1.0], [100.0, 60.0, 41.0]).unwrap();
    let a = det::atan(0.02_f64);
    let (c, s) = (det::cos(a), det::sin(a));
    hollow
        .transform(&[
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
    let outer = make_box([-52.0, -32.0, 0.0], [104.0, 64.0, 42.0]).unwrap();
    boolean(outer, hollow, BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// An inside round counts as a fill only along the band it runs beside: on `tapered_hollow`
/// its edge is skew to the outer round's, so it is no fill and the R7 pair is refused for
/// breaking through, as the outer round alone is.
#[test]
#[allow(clippy::disallowed_methods)] // the test's own placement, not ops computing geometry
fn an_inside_round_skew_to_the_outer_one_is_no_fill() {
    all_fixes();
    let wall = tapered_hollow();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    // the turned wall y = 30 meets x = 0 at y = 30 / cos a = 30 sqrt(1 + 0.02^2)
    let inside = edge_at(&wall, [0.0, 30.0 * 1.0004_f64.sqrt(), 40.0]);
    for edges in [&[top][..], &[top, inside][..]] {
        match round(&wall, edges, &[7.0]) {
            Err(Error::TooLarge {
                why: Some(RoomReason::BreaksThrough),
                ..
            }) => {}
            other => panic!("{other:?}"),
        }
    }
}

/// #1668's box with the hollow's +x end cut oblique, on the plane x - 0.1 y = 46 (square to
/// neither wall's corner edge).
#[allow(clippy::disallowed_methods)] // the test's own placement, not ops computing geometry
fn oblique_end() -> Body {
    use solidcore_ops::boolean::boolean;
    let hollow = make_box([-50.0, -30.0, -1.0], [110.0, 60.0, 41.0]).unwrap();
    let cut = make_box([0.0, -100.0, -10.0], [100.0, 200.0, 70.0]).unwrap();
    // turned about z so that its x = 0 face has normal (1, -0.1, 0), then moved to x = 46
    let a = det::atan(-0.1_f64);
    let (c, s) = (det::cos(a), det::sin(a));
    // (rows are the images of x, y, z and the move, as `Body::transform` reads them)
    cut.transform(&[
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [46.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let hollow = boolean(hollow, cut, BooleanOp::Subtract).unwrap().unwrap();
    let outer = make_box([-52.0, -32.0, 0.0], [104.0, 64.0, 42.0]).unwrap();
    boolean(outer, hollow, BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// The inside round counts as a fill only where both ends of its edge are closed by a plane
/// square to it. On `oblique_end` the inside corner edge (30, 40) ends at x = 49 on the
/// oblique end face, so the inside round is no fill and the R7 outer round is refused with the
/// fit of the outer round alone, 4 + 2 sqrt2.
#[test]
fn an_inside_round_ending_on_an_oblique_face_is_no_fill() {
    all_fixes();
    let wall = oblique_end();
    // 104 64 42 less the hollow's 40 high (96 + 0.1 y) over y in [-30, 30]
    close(volume(&wall), 279_552.0 - 40.0 * 96.0 * 60.0);
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    breaks_through(
        round(&wall, &[top, inside], &[7.0]),
        4.0 + 2.0 * std::f64::consts::SQRT_2,
        [0.0, 32.0, 42.0],
    );
}

/// What a session on #1668's box with the top and inside edges rounded R7 says, with `setup`
/// run on it first: whether it built, the error code it noted, and its no-room reason.
fn session_on_thin_wall(
    setup: impl FnOnce(
        &mut solidcore::blend::fillet_executive::session::FilletSession,
        solidcore::topology::VertexId,
    ),
) -> (
    bool,
    i32,
    Option<solidcore::blend::fillet_executive::p3::NoRoomReason>,
) {
    use solidcore::blend::fillet_executive::session::FilletSession;
    let wall = thin_walled();
    let picks = [
        edge_at(&wall, [0.0, 32.0, 42.0]),
        edge_at(&wall, [0.0, 30.0, 40.0]),
    ];
    let end = vertex_at(&wall, [50.0, 30.0, 40.0]);
    let (mut k, mut brep) = kernel_copy(&wall);
    let edges: Vec<solidcore::EdgeId> = picks.iter().map(|&i| brep.edge_list()[i]).collect();
    let end = brep.vertex_list()[end];
    let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &edges, 7.0, 1, 1, 1.0)
        .unwrap();
    setup(&mut session, end);
    let done = session.do_fillet(&mut k);
    let info = session.error_info();
    (done.is_ok(), info.code, info.no_room_reason())
}

/// The inside round counts as a fill neither in the Roll-over's second pass (whose concave
/// overflow across a cliff is accepted and trimmed, so its rounds need not add their whole
/// section) nor where an end of its edge is a set-back vertex (a set-back patch replaces the
/// round there). On the session that builds #1668's R7 pair, each refuses it for breaking
/// through. The set-back's other door, a set-back fill asked at that end, never reaches the
/// judgement: the corners step refuses a fill asked at a vertex with one blend first, so the
/// pair is refused, but not for breaking through.
#[test]
fn an_inside_round_is_no_fill_in_a_roll_over_pass_or_at_a_set_back_end() {
    use solidcore::blend::fillet_executive::p3::NoRoomReason;
    use solidcore::blend::fillet_geom::p1::FilletErrorType;
    all_fixes();
    let no_room = FilletErrorType::NoRoom as i32;
    assert_eq!(session_on_thin_wall(|_, _| {}), (true, 0, None));
    assert_eq!(
        session_on_thin_wall(|s, end| s.set_roll_over_pass(vec![end])),
        (false, no_room, Some(NoRoomReason::BreaksThrough))
    );
    assert_eq!(
        session_on_thin_wall(|s, end| {
            s.add_set_back_vertex(end);
        }),
        (false, no_room, Some(NoRoomReason::BreaksThrough))
    );
    assert_eq!(
        session_on_thin_wall(|s, end| s.set_set_back_fill(end, 3.0, 0.0, false)),
        (false, FilletErrorType::CornerUnsupported as i32, None)
    );
}

/// A round whose solver the session allocated (offsets and a circular generator) but never
/// loaded is never built, so it fills nothing: #1668's R7 on the top edge alone, with such a
/// solver R7 on the inside edge, is refused for breaking through, as without it.
#[test]
fn an_inside_round_never_loaded_is_no_fill() {
    use solidcore::blend::fillet_executive::p3::NoRoomReason;
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let (mut k, mut brep) = kernel_copy(&wall);
    let (top, inside) = (brep.edge_list()[top], brep.edge_list()[inside]);
    let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[top], 7.0, 1, 1, 1.0)
        .unwrap();
    let generator = session
        .add_surface_generator(FilletSurfaceGenerator::circular(
            false, 1.0e-3, false, false,
        ))
        .unwrap();
    let edgeuse = session.blend_edgeuse(&k, inside).unwrap().unwrap();
    let tol = session.target().tolerance();
    let degree = std::f64::consts::PI / 180.0;
    let solver = session
        .new_constant_radius_solver(&k, edgeuse, 7.0, tol, 30.0 * degree, 2.0 * degree)
        .unwrap();
    session.set_solver_generator(solver, generator).unwrap();
    assert!(session.do_fillet(&mut k).is_err());
    assert_eq!(
        session.error_info().no_room_reason(),
        Some(NoRoomReason::BreaksThrough)
    );
}
