//! WR-300 addendum (the fork's tangency-labs/smlib@465b62b, the rule of @1d06159):
//! `SmFilletSolver::RailRailIntersect` holds the variable radii at their end values
//! (`SmTemporaryRadiusChange`) only at a corner calculated as constant radius, as the
//! rail/side-edge search does. Upstream holds them at every corner: two rounds 0.5 -> 2
//! along a 10 box's top edges meeting at (10, 10, 10) (the vertical edge there not
//! rounded) had their rails meet at (10, 8, 8), radius 2, where the law gives 1.7, and
//! the trace from there, its spans held to the law
//! (`fillet_variable_span_follows_fillet`, 4438859), failed with 1001: the `SM_API` test
//! `TestSmVariableRadiusFillet`, which rounds four top edges. This is now the only path
//! (the flag `fillet_retry_radius_as_first` is retired).
//!
//! The fork's check: a sound body removing more than one round alone,
//! `(1 - pi/4) * 17.5 = 3.7555` (the mean of `r^2` along a linear law 0.5 -> 2 is
//! `(0.25 + 1 + 4) / 3 = 1.75`), and less than both, 6.8226. And the closed form of the
//! rails' meeting point: on the top face the round along `x = 10` touches at
//! `x = 10 - r(y)` and the one along `y = 10` at `y = 10 - r(x)`, `r(s) = 0.5 + 0.15 s`,
//! so they meet at `x = y = 9.5 / 1.15 = 190 / 23`, where both balls have the law's
//! radius `r(190 / 23)`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::variable_radius_fillet;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge from `p` to `q`.
fn edge_from_to(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            c.evaluate_point(k, ivl.min()).unwrap().distance(p) < 1e-9
                && c.evaluate_point(k, ivl.max()).unwrap().distance(q) < 1e-9
        })
        .expect("edge")
}

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_variable_span_follows_fillet = true;
    k
}

/// The 10 box with its top edges x = 10 and y = 10 rounded 0.5 -> 2 towards their
/// common vertex (10, 10, 10).
fn two_rounds(k: &mut Kernel) -> solidcore::Result<Brep> {
    let mut b = create_box(k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let corner = Point3::new(10.0, 10.0, 10.0);
    let edges = [
        edge_from_to(k, &b, Point3::new(10.0, 0.0, 10.0), corner),
        edge_from_to(k, &b, Point3::new(0.0, 10.0, 10.0), corner),
    ];
    variable_radius_fillet(k, &mut b, &edges, 0.5, 2.0, 1, 1)?;
    Ok(b)
}

#[test]
fn two_variable_rounds_meet_at_their_law_radii() {
    let mut k = kernel();
    let mut b = two_rounds(&mut k).unwrap();
    let removed = 1000.0 - volume(&k, &mut b);
    let one = (1.0 - core::f64::consts::FRAC_PI_4) * 17.5;
    assert!(
        removed > one && removed < 6.8226,
        "removed {removed}, one round alone {one}"
    );
}

/// The rails meet on the top face where both balls have the law's radius: the vertex
/// at `(190 / 23, 190 / 23, 10)`, not upstream's `(8, 8, 10)` of the end radius 2.
#[test]
fn the_rails_meet_where_both_balls_have_the_law_radius() {
    let mut k = kernel();
    let b = two_rounds(&mut k).unwrap();
    let meet = Point3::new(190.0 / 23.0, 190.0 / 23.0, 10.0);
    let nearest = b
        .vertices()
        .ids()
        .map(|v| vertex::point(&b, v).unwrap().distance(meet))
        .fold(f64::INFINITY, f64::min);
    assert!(nearest < 1e-9, "nearest vertex {nearest} from {meet:?}");
}

/// `TestSmVariableRadiusFillet`'s four top edges, each 0.5 -> 2 along its own direction:
/// the opposite edges' rounds are disjoint, so the body loses more than two rounds and
/// less than four.
#[test]
fn four_top_edges_round() {
    let mut k = kernel();
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let p = |x: f64, y: f64| Point3::new(x, y, 10.0);
    let edges = [
        edge_from_to(&k, &b, p(10.0, 0.0), p(10.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 10.0), p(10.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 0.0), p(0.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 0.0), p(10.0, 0.0)),
    ];
    variable_radius_fillet(&mut k, &mut b, &edges, 0.5, 2.0, 1, 1).unwrap();
    let removed = 1000.0 - volume(&k, &mut b);
    let one = (1.0 - core::f64::consts::FRAC_PI_4) * 17.5;
    assert!(
        removed > 2.0 * one && removed < 4.0 * one,
        "removed {removed}, one round alone {one}"
    );
}

/// One corner station of the four-edge case in closed form: a law ball of one round at
/// its own edge position, its foot on the top face (a vertex of the result: both rounds'
/// rails meet there) and its foot on its side face (on the round's rail).
struct CornerBall {
    centre: [f64; 3],
    radius: f64,
    top_foot: [f64; 3],
    side_foot: [f64; 3],
}

/// The law `r(s) = 0.5 + 0.15 s` of every edge of the four-edge case.
fn law(s: f64) -> f64 {
    0.5 + 0.15 * s
}

/// The eight corner balls of the four-edge case (two per corner), as exact rationals.
///
/// Edges (each rounded along its own direction, `s` its coordinate): A along `x = 10`
/// (`s = y`), B along `y = 10` (`s = x`), C along `x = 0` (`s = y`), D along `y = 0`
/// (`s = x`), all on the top face `z = 10`. A ball of a round has radius `r(s)` and sits
/// `r(s)` from both its faces; its feet are its nearest points on them. At a corner the
/// two rounds' rails on the top face cross, so the two balls share their top foot:
/// - (10, 10, 10), A and B: `x = 10 - r(y)`, `y = 10 - r(x)`: `x = y = 190/23`, both
///   radii `r(190/23) = 40/23`, one centre (190/23, 190/23, 190/23);
/// - (0, 10, 10), C and B: `x = r(y)`, `y = 10 - r(x)`: `x = 1.925 / 1.0225 = 770/409`,
///   `y = 3770/409`; C's radius `r(3770/409) = 770/409`, B's `r(770/409) = 320/409`;
/// - (10, 0, 10), A and D: the mirror, `(x, y) = (3770/409, 770/409)`;
/// - (0, 0, 10), C and D: `x = r(y)`, `y = r(x)`: `x = y = 0.5 / 0.85 = 10/17`, both radii
///   `10/17`, one centre (10/17, 10/17, 160/17).
fn corner_balls() -> [CornerBall; 8] {
    let (a, ra) = (190.0 / 23.0, 40.0 / 23.0);
    let (lo, hi) = (770.0 / 409.0, 3770.0 / 409.0);
    let (r_big, r_small) = (770.0 / 409.0, 320.0 / 409.0);
    let (q, rq) = (10.0 / 17.0, 10.0 / 17.0);
    let ball = |centre: [f64; 3], radius: f64, side_foot: [f64; 3]| CornerBall {
        centre,
        radius,
        top_foot: [centre[0], centre[1], 10.0],
        side_foot,
    };
    [
        // (10, 10, 10): A (side x = 10) and B (side y = 10)
        ball([10.0 - ra, a, 10.0 - ra], ra, [10.0, a, 10.0 - ra]),
        ball([a, 10.0 - ra, 10.0 - ra], ra, [a, 10.0, 10.0 - ra]),
        // (0, 10, 10): C (side x = 0) and B (side y = 10)
        ball([r_big, hi, 10.0 - r_big], r_big, [0.0, hi, 10.0 - r_big]),
        ball(
            [lo, 10.0 - r_small, 10.0 - r_small],
            r_small,
            [lo, 10.0, 10.0 - r_small],
        ),
        // (10, 0, 10): A (side x = 10) and D (side y = 0)
        ball(
            [10.0 - r_small, lo, 10.0 - r_small],
            r_small,
            [10.0, lo, 10.0 - r_small],
        ),
        ball([hi, r_big, 10.0 - r_big], r_big, [hi, 0.0, 10.0 - r_big]),
        // (0, 0, 10): C (side x = 0) and D (side y = 0)
        ball([rq, q, 10.0 - rq], rq, [0.0, q, 10.0 - rq]),
        ball([q, rq, 10.0 - rq], rq, [q, 0.0, 10.0 - rq]),
    ]
}

/// The four top edges, each rounded 0.5 -> 2 along its own direction.
fn four_top_edges(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let p = |x: f64, y: f64| Point3::new(x, y, 10.0);
    let edges = [
        edge_from_to(k, &b, p(10.0, 0.0), p(10.0, 10.0)),
        edge_from_to(k, &b, p(0.0, 10.0), p(10.0, 10.0)),
        edge_from_to(k, &b, p(0.0, 0.0), p(0.0, 10.0)),
        edge_from_to(k, &b, p(0.0, 0.0), p(10.0, 0.0)),
    ];
    variable_radius_fillet(k, &mut b, &edges, 0.5, 2.0, 1, 1).unwrap();
    b
}

/// Each corner ball is the law's ball at its own position (radius `r(s)`, `r(s)` from
/// both faces, feet at `r(s)` from the centre), each top foot is a vertex of the result
/// to 1e-12, and each side foot lies on a round face of the result to 1e-9. (The
/// distance to the face is the closest point the kernel finds on it: never below the true
/// distance, so the check is conservative.) Upstream's hold (the radii at their end
/// values, 2 or 0.5) puts the top feet at (8, 8), (2, 9.5), (9.5, 2) and (0.5, 0.5)
/// instead, where `r(2) = 0.8`, not 0.5, and `r(8) = 1.7`, not 2.
fn check_corner_balls(k: &Kernel, b: &Brep) {
    let vertices: Vec<Point3> = b
        .vertices()
        .ids()
        .map(|v| vertex::point(b, v).unwrap())
        .collect();
    let rounds: Vec<_> = b
        .faces()
        .ids()
        .filter_map(|f| face::surface(b, f).unwrap().cloned())
        .filter(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
        .collect();
    let pt = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    for ball in corner_balls() {
        let (c, r) = (pt(ball.centre), ball.radius);
        // the law's ball at its own position: the position is the coordinate along its
        // edge, the centre's other coordinates are `r` in from the faces
        let along = if (ball.side_foot[0] - 0.0).abs() < 1e-12
            || (ball.side_foot[0] - 10.0).abs() < 1e-12
        {
            c.y
        } else {
            c.x
        };
        assert!(
            (law(along) - r).abs() < 1e-12,
            "{:?}: r {r} law {}",
            ball.centre,
            law(along)
        );
        assert!((10.0 - c.z - r).abs() < 1e-12);
        assert!((c.distance(pt(ball.top_foot)) - r).abs() < 1e-12);
        assert!((c.distance(pt(ball.side_foot)) - r).abs() < 1e-12);
        let top = pt(ball.top_foot);
        let nearest = vertices
            .iter()
            .map(|v| v.distance(top))
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < 1e-12,
            "no vertex at {top:?} (nearest {nearest}); vertices {vertices:?}"
        );
        let side = pt(ball.side_foot);
        let on_round = rounds
            .iter()
            .map(|s| {
                let uv = surface_closest_point(k, s, side).unwrap().uvs[0];
                s.evaluate_point(k, uv).unwrap().distance(side)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            on_round < 1e-9,
            "side foot {side:?} is {on_round} from the rounds"
        );
    }
}

/// [`check_corner_balls`] with `fillet_variable_span_follows_fillet` on (the trace needs
/// it to pass the old corner points, which fail with 1001 there).
#[test]
fn four_top_edges_meet_at_the_laws_closed_forms() {
    let mut k = kernel();
    let b = four_top_edges(&mut k);
    check_corner_balls(&k, &b);
}

/// [`check_corner_balls`] on the default kernel. (The old code refused this case there
/// too, with 1001: its corner points are not on the law's balls.)
#[test]
fn four_top_edges_meet_at_the_laws_closed_forms_on_the_default_kernel() {
    let mut k = Kernel::default();
    let b = four_top_edges(&mut k);
    check_corner_balls(&k, &b);
}

/// `TestSmVariableRadiusFillet` as the `SM_API` test runs it, on the default kernel: the
/// box's first four edges (two top edges meeting at (10, 10, 10), the top edge along
/// `x = 0`, and a bottom edge), each rounded 0.5 -> 2 along its own direction. Its two top
/// corners are (10, 10, 10), where both rounds are at `r = 2`, and (0, 10, 10), where one
/// is at 0.5 and one at 2. The rails meet at the law balls' common feet
/// (190/23, 190/23, 10) and (770/409, 3770/409, 10) ([`corner_balls`]). The old code built
/// this case on the default kernel with those corners at the end radii, (8, 8, 10) and
/// (2, 9.5, 10), off the laws' balls (observed on this branch's parent); neither is a
/// vertex now.
#[test]
fn the_sm_api_case_meets_at_the_laws_closed_forms_on_the_default_kernel() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let edges: Vec<EdgeId> = solidcore::topology::brep::p1::edges(&b, None)
        .unwrap()
        .into_iter()
        .take(4)
        .collect();
    variable_radius_fillet(&mut k, &mut b, &edges, 0.5, 2.0, 1, 1).unwrap();
    let points: Vec<Point3> = b
        .vertices()
        .ids()
        .map(|v| vertex::point(&b, v).unwrap())
        .collect();
    let nearest = |e: Point3| {
        points
            .iter()
            .map(|q| q.distance(e))
            .fold(f64::INFINITY, f64::min)
    };
    for e in [
        Point3::new(190.0 / 23.0, 190.0 / 23.0, 10.0),
        Point3::new(770.0 / 409.0, 3770.0 / 409.0, 10.0),
    ] {
        assert!(
            nearest(e) < 1e-12,
            "no vertex at {e:?}; vertices {points:?}"
        );
    }
    for old in [Point3::new(8.0, 8.0, 10.0), Point3::new(2.0, 9.5, 10.0)] {
        assert!(nearest(old) > 0.1, "a vertex at the old corner {old:?}");
    }
}
