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
//!
//! The rail's point judged is where the round's contact line is at the edge's end (its crossing
//! of the plane through the vertex square to the edge), not the rail's own end: a rail runs on
//! past the edge's ends, and an arc's rail turns away from the side edge's line as it does
//! (#17710, the last test).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
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

/// The cliff overflow's flags, as SolidCraft runs them (the trim at creases is unconditional).
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_reaches_rails = true;
    k
}

fn stepped_block(k: &mut Kernel, profile: &[[f64; 2]]) -> Brep {
    let cs: Vec<Curve> = profile
        .iter()
        .zip(profile.iter().cycle().skip(1))
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
    round_top_front_of(&PROFILE, r)
}

fn round_top_front_of(profile: &[[f64; 2]], r: f64) -> Outcome {
    let mut k = kernel();
    let mut b = stepped_block(&mut k, profile);
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

/// The stepped block sheared by x' = x - z + 12: its riser G runs from (20,0,12) down to
/// P = (22,0,10), the plane x + z = 32, oblique to the edge. A round of radius r has its rail
/// on F at z = 12 - r, which meets G's plane at x = 20 + r: past P once r > 2 (at r = 3,
/// (23,0,9), where the plane square to the edge at the vertex, x = 20, holds (20,0,9), short
/// of P along the side). The end is cut by G's plane and the start by the left face's plane
/// x + z = 12, parallel to it 20 apart along x, so the round takes off its straight section
/// over 20 as on the unsheared block: `r^2 (1 - pi/4) 20`.
const SHEARED: [[f64; 2]; 6] = [
    [12.0, 0.0],
    [52.0, 0.0],
    [42.0, 10.0],
    [22.0, 10.0],
    [20.0, 12.0],
    [0.0, 12.0],
];

#[test]
fn a_round_past_an_oblique_end_face_is_cut_by_its_plane() {
    // the sheared block's volume: the step's 40 x 10 parallelogram and the 20 x 2 one above
    let block = (40.0 * 10.0 + 20.0 * 2.0) * DEPTH;
    for r in [1.5, 3.0] {
        let want = block - straight(r);
        match round_top_front_of(&SHEARED, r) {
            Outcome::Built(v) => assert!(
                (v - want).abs() < 1e-6 * block,
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

/// #17710: a ring turned about the x axis, its inner face the cone r = 50 + x over x in
/// [-4, 10] (half-angle 45 degrees), meeting a cylinder r = 45.646 below through a concave
/// round 0.5 (tangent to the cone at (-4, 46)), and a wedge turned 10 degrees about the axis
/// (from 25 to 35) filling under the cone and the round from x = 0 down: its face beside the
/// cone leaves (0, 50) 17 degrees steeper than the cone, so the two meet at 17 degrees along an
/// arc of radius 50 (a concave seam). Each end of that arc is a vertex of the wedge's end
/// plane (through the axis), whose side edge on the cone runs down to P = (-4, 46), where the
/// end plane meets the round's face in a concave edge. The cone runs on past the end plane,
/// so the rail of a 0.5 round on it crosses no side edge; its own end runs on round the axis
/// (the exact round's trace reaches far past the edge's ends) and projected past P on the side
/// edge's line, so the round was taken for one whose end section starts past P and refused at
/// every radius ("a rail leaves the face it rolls on ... and adds material"). It meets the end
/// plane short of the vertex, its end section is on the end face, and it builds: the closed
/// form ([`wedge_round`]), a manifold solid, one face more, `check_brep` at Level2.
fn ring_with_wedge(k: &mut Kernel) -> Brep {
    let s2 = std::f64::consts::FRAC_1_SQRT_2;
    let (t, c) = ([-4.0, 46.0], [-4.0 + 0.5 * s2, 46.0 - 0.5 * s2]);
    let e = [c[0] - 0.5, c[1]];
    let turn = (225.0_f64 + 17.0).to_radians();
    let r = [4.0 * turn.cos(), 50.0 + 4.0 * turn.sin()];
    // the profile in the half-plane at `deg` about the x axis: (x, radius)
    let profile = |k: &mut Kernel, deg: f64, points: &[[f64; 2]]| -> Vec<Curve> {
        let (cos, sin) = (deg.to_radians().cos(), deg.to_radians().sin());
        let at = |p: [f64; 2]| Point3::new(p[0], p[1] * cos, p[1] * sin);
        let radial = Vec3::new(0.0, cos, sin);
        let frame = Axis2Placement::new(at(c), Vec3::new(1.0, 0.0, 0.0), radial);
        let round = Ellipse::create_canonical_circle(
            k,
            &frame,
            0.5,
            Some(&Extent1d::from_min_max(135.0, 180.0)),
            None,
        )
        .unwrap();
        let mut cs: Vec<Curve> = points
            .windows(2)
            .map(|w| Curve::from(create_line_segment(at(w[0]), at(w[1])).unwrap()))
            .collect();
        cs.push(Curve::from(round));
        cs
    };
    // T up the cone to the top, round the ring's outside to E, the round from E back to T
    let base = profile(
        k,
        0.0,
        &[
            t,
            [10.0, 60.0],
            [10.0, 70.0],
            [-10.0, 70.0],
            [-10.0, e[1]],
            e,
        ],
    );
    let refs: Vec<Option<&Curve>> = base.iter().map(Some).collect();
    let ring = create_rotational_sweep(
        k,
        &refs,
        Point3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        360.0,
        true,
    )
    .unwrap();
    // T up the cone to (0, 50), down the wedge's face to R and across to E, the round to T
    let wedge = profile(k, 25.0, &[t, [0.0, 50.0], r, e]);
    let refs: Vec<Option<&Curve>> = wedge.iter().map(Some).collect();
    let wedge =
        create_rotational_sweep(k, &refs, Point3::ZERO, Vec3::new(1.0, 0.0, 0.0), 10.0, true)
            .unwrap();
    boolean_union(k, ring, wedge).unwrap()
}

/// The material the 0.5 round adds to the seam: its section, between the arc and the faces
/// meeting at (0, 50) (the cone along 45 degrees, the wedge's face 17 degrees steeper),
/// turned 10 degrees about the axis (Pappus): the kite of the corner, its contacts and the
/// arc's centre less the arc's sector, by its moment about the axis.
fn wedge_round() -> f64 {
    let (r, d) = (0.5_f64, 17.0_f64.to_radians());
    let along = |deg: f64| [deg.to_radians().cos(), deg.to_radians().sin()];
    let (p, u1, u2) = ([0.0, 50.0], along(45.0), along(242.0));
    let half = (std::f64::consts::PI - d) / 2.0;
    let bisector = [
        (u1[0] + u2[0]) / 2.0 / half.cos(),
        (u1[1] + u2[1]) / 2.0 / half.cos(),
    ];
    let at = |u: [f64; 2], t: f64| [p[0] + t * u[0], p[1] + t * u[1]];
    let (a, b) = (at(u1, r / half.tan()), at(u2, r / half.tan()));
    let c = at(bisector, r / half.sin());
    // a triangle's area and moment about the axis (its centroid's radius)
    let tri = |q: [f64; 2], s: [f64; 2]| {
        let area = ((q[0] - p[0]) * (s[1] - p[1]) - (s[0] - p[0]) * (q[1] - p[1])).abs() / 2.0;
        area * (p[1] + q[1] + s[1]) / 3.0
    };
    let sector = r * r * d / 2.0;
    let centroid = c[1] - 4.0 * r * (d / 2.0).sin() / (3.0 * d) * bisector[1];
    (tri(a, c) + tri(c, b) - sector * centroid) * 10.0_f64.to_radians()
}

#[test]
fn a_concave_round_whose_face_runs_on_past_the_end_face_builds() {
    let mut k = kernel();
    let mut b = ring_with_wedge(&mut k);
    let v0 = volume(&k, &mut b);
    let faces = b.face_list().len();
    // the seam: the arc at x = 0 of radius 50, 10 degrees long
    let on_seam = |p: Point3| p.x.abs() < 1e-6 && (p.y.hypot(p.z) - 50.0).abs() < 1e-6;
    let seam: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let ivl = edge::interval(&b, e).unwrap();
            let c = edge::curve(&b, e).unwrap().unwrap();
            [ivl.min(), ivl.mid(), ivl.max()]
                .iter()
                .all(|&t| on_seam(c.evaluate_point(&k, t).unwrap()))
        })
        .collect();
    let [e] = seam[..] else {
        panic!("one seam edge: {seam:?}")
    };
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], 0.5, 1, 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    assert!(result.is_ok(), "refused: {}", info.comment);
    assert!(is_manifold_solid(&b).unwrap(), "not a manifold solid");
    assert_eq!(b.face_list().len(), faces + 1, "one round face");
    let added = volume(&k, &mut b) - v0;
    assert!(
        (added - wedge_round()).abs() < 1e-3 * wedge_round(),
        "added {added}, closed form {}",
        wedge_round()
    );
    b.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "check_brep: {lines:?}");
}

/// The part of #17780: a prism whose section is a circle of radius 12.5 about the y axis cut
/// by the chord at z = -7.5, swept 20 along y, with a block 12 wide and 3.5 high standing 5 out
/// of its front cap (y = 0) on the chord's middle, its underside flush with the chord's face.
/// The cap is F and the arc's cylinder G, one face whose own domain is the arc. The cap's chord
/// edge from the arc's end V = (-10, 0, -7.5) runs to P = (-6, 0, -7.5), where the block's wall
/// rises. A round of radius r on the arc has its rail on F on the circle of radius 12.5 - r,
/// which meets the chord's line past P, under the block, once 12.5 - r is below |P| =
/// sqrt(92.25): such a round is refused naming that radius.
fn d_with_block(k: &mut Kernel) -> Brep {
    let frame = Axis2Placement::new(
        Point3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let end = (0.6_f64).asin().to_degrees();
    let arc = Ellipse::create_canonical_circle(
        k,
        &frame,
        12.5,
        Some(&Extent1d::from_min_max(-end, 180.0 + end)),
        None,
    )
    .unwrap();
    let chord =
        create_line_segment(Vec3::new(-10.0, 0.0, -7.5), Vec3::new(10.0, 0.0, -7.5)).unwrap();
    let cs = [Curve::from(arc), Curve::from(chord)];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let d = create_linear_sweep(k, &refs, Vec3::new(0.0, 1.0, 0.0), DEPTH, true).unwrap();
    let block = create_box(k, Point3::new(-6.0, -5.0, -7.5), 12.0, 5.0, 3.5).unwrap();
    boolean_union(k, d, block).unwrap()
}

fn round_d_with_block(r: f64) -> Outcome {
    // as SolidCraft runs it: the swept arc a cylinder, the round's offset surfaces continued
    // as themselves (so the rail past V is the exact circle on F)
    let mut k = kernel();
    k.fixes.linear_sweep_arc_is_cylinder = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_extension_as_itself = true;
    let mut b = d_with_block(&mut k);
    let top = Point3::new(0.0, 0.0, 12.5);
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(&b, e).unwrap();
            let c = edge::curve(&b, e).unwrap().unwrap();
            [ivl.min(), ivl.mid(), ivl.max()].iter().all(|&t| {
                let p = c.evaluate_point(&k, t).unwrap();
                p.y.abs() < 1e-9 && (p.x.hypot(p.z) - 12.5).abs() < 1e-9
            }) && c.evaluate_point(&k, ivl.mid()).unwrap().distance(top) < 1e-6
        })
        .expect("the arc on the front cap");
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
            b.use_arena_naming().unwrap();
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
            Outcome::Built(0.0)
        }
        Err(_) => Outcome::Refused(info.comment),
    }
}

/// #17780: the ball touching F at P and touching G is measured to G's cylinder, not to G's
/// face, whose own domain ends at V: there the centre's foot stopped at V, the ball came out
/// larger than the round's, and the End, finding a rail past P that did not reach P, failed as
/// "Corner Edge Computation Failure" instead of naming the radius that fits.
#[test]
fn a_round_on_an_arc_running_under_a_block_names_the_radius_that_fits() {
    let fits = 12.5 - 92.25_f64.sqrt();
    assert!(
        matches!(round_d_with_block(2.5), Outcome::Built(_)),
        "2.5 stays short of P and builds"
    );
    let Outcome::Refused(comment) = round_d_with_block(3.0) else {
        panic!("3 runs past P under the block: built");
    };
    let named = comment
        .split("a radius below ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("no fitting radius named: {comment}"));
    assert!(
        (named - fits).abs() < 1e-5,
        "named {named}, closed form {fits}: {comment}"
    );
}
