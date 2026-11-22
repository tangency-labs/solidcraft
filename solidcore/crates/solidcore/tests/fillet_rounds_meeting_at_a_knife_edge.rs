//! The two top edges that meet at a knife-edge tip rounded one after the other (#2695): the
//! same body in either order.
//!
//! The arc part: a 41 1/6 x 21 rectangle and, beside it, what a circle of radius 21 about
//! (62 1/6, -21) leaves against the top line it touches, extruded 10. Past where the rounds'
//! contact lines on the top face cross, the back's round (a cylinder) and the arc's (a torus) cut
//! each other along a ridge from P = (62 1/6 - sqrt(84 r), -r, 10) to T = (62 1/6, 0, 10 - r), on
//! y = -dx^2 / 84, z = 10 - r + sqrt(r^2 - (y + r)^2), dx = x - 62 1/6. The normals are parallel
//! at both ends, so each is a branch point: at T two branches cross at 2 atan(sqrt(r / 42)).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const X0: f64 = 41.0 + 1.0 / 6.0;
const CX: f64 = X0 + 21.0;

fn arc_part(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let mut arc = create_arc(Point3::new(CX, -21.0, 0.0), 21.0, 90.0, 180.0).unwrap();
    let ivl = arc.natural_interval();
    arc.reverse_parameterization(&ivl).unwrap();
    let curves = [
        Curve::from(create_line_segment(p(0.0, 0.0), p(0.0, -21.0)).unwrap()),
        Curve::from(create_line_segment(p(0.0, -21.0), p(X0, -21.0)).unwrap()),
        Curve::from(arc),
        Curve::from(create_line_segment(p(CX, 0.0), p(0.0, 0.0)).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

fn pick(k: &Kernel, b: &Brep, at: Point3) -> EdgeId {
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let Ok(d) = c.drop_point(
            k,
            &ivl,
            at,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        ) else {
            continue;
        };
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.expect("an edge through the point").0
}

fn round(k: &mut Kernel, b: &mut Brep, at: Point3, r: f64) {
    let e = pick(k, b, at);
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(
            k,
            &[e],
            &[r],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(k, None).unwrap();
        s.do_filleting(k, None).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
}

/// The volume after the edges through `first` and then `then`, the body checked.
fn rounded(first: Point3, then: Point3, r: f64) -> (Brep, Kernel, f64) {
    let mut k = kernel();
    let mut b = arc_part(&mut k);
    round(&mut k, &mut b, first, r);
    round(&mut k, &mut b, then, r);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "r {r}: {lines:?}");
    let v = volume(&k, &mut b);
    (b, k, v)
}

fn arc_top() -> Point3 {
    let a = std::f64::consts::FRAC_PI_4;
    Point3::new(CX - 21.0 * a.cos(), -21.0 + 21.0 * a.sin(), 10.0)
}

const BACK: Point3 = Point3::new(30.0, 0.0, 10.0);

#[test]
fn either_order_is_the_one_body() {
    for r in [0.5, 1.0, 2.0, 3.0] {
        let (_, _, a) = rounded(arc_top(), BACK, r);
        let (_, _, b) = rounded(BACK, arc_top(), r);
        assert!(
            (a - b).abs() <= 1e-6 * a,
            "r {r}: arc first {a}, back first {b}"
        );
    }
}

/// The volume at r 2 against an independent quadrature of the part with both rounds' material
/// taken off (the kept top height the least of the two rounds'; no kernel).
#[test]
fn back_then_arc_at_r_2_is_the_quadrature() {
    let (_, _, v) = rounded(BACK, arc_top(), 2.0);
    assert!((v - 9518.6924).abs() < 1e-3, "{v}");
}

/// The ridge's edge, from P to T, in the body.
fn ridge(b: &Brep, k: &Kernel, r: f64) -> EdgeId {
    let p = Point3::new(CX - (84.0_f64 * r).sqrt(), -r, 10.0);
    let t = Point3::new(CX, 0.0, 10.0 - r);
    get_edges(b)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let c = edge::curve(b, e).unwrap().unwrap();
            let iv = edge::interval(b, e).unwrap();
            let (s, z) = (
                c.evaluate_point(k, iv.min()).unwrap(),
                c.evaluate_point(k, iv.max()).unwrap(),
            );
            (s.distance(p) < 1e-6 && z.distance(t) < 1e-6)
                || (s.distance(t) < 1e-6 && z.distance(p) < 1e-6)
        })
        .expect("an edge from P to T")
}

/// The point of the closed-form ridge at `x`.
fn on_ridge(x: f64, r: f64) -> Point3 {
    let dx = x.min(CX) - CX;
    let y = -dx * dx / 84.0;
    let u = y + r;
    Point3::new(x.min(CX), y, 10.0 - r + (r * r - u * u).max(0.0).sqrt())
}

/// The distance from `q` to the closed-form ridge: golden section over x within 0.05 of `q`'s,
/// the ridge being a graph over x.
fn off_ridge(q: Point3, r: f64) -> f64 {
    let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
    let dist = |x: f64| on_ridge(x, r).distance(q);
    let (mut lo, mut hi) = (q.x - 0.05, (q.x + 0.05).min(CX));
    for _ in 0..100 {
        let (x1, x2) = (hi - golden * (hi - lo), lo + golden * (hi - lo));
        if dist(x1) < dist(x2) {
            hi = x2;
        } else {
            lo = x1;
        }
    }
    dist(0.5 * (lo + hi))
}

/// Below this angle between the rounds the shape isn't checked: the point both fix is set by
/// their curvature there, not by the wedge the bound below is taken from.
const SHAPE_CHECK_MIN_ANGLE_DEG: f64 = 10.0;

/// The ridge's edge in the body, against the two exact surfaces, each order at each radius.
///
/// Every sample is within the edge's tolerance of both: the back's cylinder about y = -r,
/// z = 10 - r, and the arc's torus about the vertical through (62 1/6, -21), tube centres at
/// radius 21 + r and height 10 - r.
///
/// Where the surfaces meet at an angle a, a point within tolerance of both is within
/// tolerance / sin(a / 2) of where they cross, the worst place being on the wedge's bisector. So
/// that bound on the distance to the closed form, a curve neither surface defines, says the edge
/// is on this ridge and not on another branch of the crossing. The rounds meet at no angle at
/// either end of the ridge, so the bound is used only at 10 degrees or more.
#[test]
fn the_ridge_is_on_both_rounds_and_follows_the_closed_form() {
    for r in [0.5, 1.0, 2.0, 3.0] {
        for (first, then) in [(arc_top(), BACK), (BACK, arc_top())] {
            let (b, k, _) = rounded(first, then, r);
            let e = ridge(&b, &k, r);
            let tol = edge::tolerance(&b, e).unwrap();
            let c = edge::curve(&b, e).unwrap().unwrap();
            let iv = edge::interval(&b, e).unwrap();
            for i in 0..=4096 {
                let q = c
                    .evaluate_point(&k, iv.evaluate(f64::from(i) / 4096.0))
                    .unwrap();

                let on_axis = Vec3::new(0.0, q.y + r, q.z - (10.0 - r));
                let to_cylinder = (on_axis.length() - r).abs();
                let rho = (q.x - CX).hypot(q.y + 21.0);
                let centre = Point3::new(
                    CX + (21.0 + r) * (q.x - CX) / rho,
                    -21.0 + (21.0 + r) * (q.y + 21.0) / rho,
                    10.0 - r,
                );
                let on_centre = q - centre;
                let to_torus = (on_centre.length() - r).abs();
                assert!(
                    to_cylinder <= tol && to_torus <= tol,
                    "r {r}: {q:?}: cylinder {to_cylinder}, torus {to_torus}, tolerance {tol}"
                );

                let sin_a =
                    on_axis.cross(on_centre).length() / (on_axis.length() * on_centre.length());
                let a = sin_a.min(1.0).asin();
                if a >= SHAPE_CHECK_MIN_ANGLE_DEG.to_radians() {
                    let off = off_ridge(q, r);
                    let bound = tol / (0.5 * a).sin();
                    assert!(
                        off <= bound,
                        "r {r}: {q:?} is {off} off the closed form, bound {bound} at {a} rad"
                    );
                }
            }
        }
    }
}
