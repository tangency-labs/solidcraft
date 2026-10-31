//! WR-669 (`fixes.fillet_exact_round_classes`), the SolidCraft fork's tangency-labs/smlib@0061c41
//! and 1b5f1e7: a constant round whose spine is a plane conic is the exact rational surface
//! of the ball's arcs, where upstream (and the flag before these commits) traces and fits it.
//!
//! - **Conic** (0061c41): a plate's top face z = 0 with an r3 peg tilted 30 degrees, its
//!   elliptic rim rounded r1. The ball's centre runs on the plane z = 1 cut through the r4
//!   cylinder about the peg's axis.
//! - **Conic pair** (1b5f1e7): an r3 pole up z and a 45 degree cone tilted 15 degrees, both
//!   tangent to the r3 ball at the origin (the cone touching it along a circle above the
//!   pole's). The concave conic where the cone leaves the pole is rounded r1; the offsets
//!   touch the r4 ball, so the spine is one of the two plane conics they meet in.
//!
//! Every sampled point of each round lies at the radius from the closed-form spine, its
//! area matches the canal surface's (integrated spectrally) to 1e-9, and the volume it
//! adds matches the divergence theorem over its closed-form boundary (the round's canal
//! surface, the face strips in closed form).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const R: f64 = 3.0;
const FILLET: f64 = 1.0;
/// How far the volume the round adds may be from the divergence theorem's: the faces next
/// to the round are trimmed by its rails through fitted UV curves (the peg's face area is
/// 2e-5 off), which the volume integrates; the traced round's is 1e-3 off. The round's own
/// area is checked to 1e-9.
const VOLUME_TOL: f64 = 2e-4;

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    volume_to(k, b, 1e-12)
}

/// The volume to `accuracy` (the tilted cone's integration fails 1001 at 1e-12).
fn volume_to(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// Turns `b` about the x axis by `tau`: z goes to (0, -sin tau, cos tau).
fn tilt(k: &mut Kernel, b: &mut Brep, tau: f64) {
    let pl = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, tau.cos(), tau.sin()),
    );
    solidcore::topology::brep::p5::transform(k, b, &pl, None, false).unwrap();
}

/// The round faces: those on B-spline surfaces, with whether each is rational.
/// The round faces' total area.
fn round_area(k: &Kernel, b: &mut Brep) -> f64 {
    let ids: Vec<_> = b
        .faces()
        .ids()
        .filter(|&f| {
            matches!(
                face::surface(b, f).unwrap().map(Surface::kind),
                Some(SurfaceKind::BSpline(_))
            )
        })
        .collect();
    ids.into_iter()
        .map(|f| solidcore::mass::integrator::properties::face_area(k, b, f, 1e-12).unwrap())
        .sum()
}

fn round_faces(b: &Brep) -> Vec<(Surface, bool)> {
    b.faces()
        .ids()
        .filter_map(|f| {
            let s = face::surface(b, f).unwrap()?;
            if let SurfaceKind::BSpline(bs) = s.kind() {
                Some((s.clone(), bs.is_rational()))
            } else {
                None
            }
        })
        .collect()
}

/// The ball along the spine: its centre and the unit directions to its contacts with the
/// first face (`a`) and the cylinder (`b`).
struct Ball {
    c: Point3,
    a: Vec3,
    b: Vec3,
}

/// The unit radial direction from the cylinder's axis at angle `theta`.
fn radial(e1: Vec3, e2: Vec3, theta: f64) -> Vec3 {
    e1 * theta.cos() + e2 * theta.sin()
}

/// The largest `| |X - spine| - r |` over a 21 x 21 grid of each round face's domain, the
/// distance to the spine minimised over its parameter (a 2048 grid, then golden section).
fn off_spine(k: &Kernel, rounds: &[(Surface, bool)], spine: &dyn Fn(f64) -> Point3) -> f64 {
    let n = 2048;
    let mut worst = 0.0f64;
    for (s, _) in rounds {
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        for i in 0..=20 {
            for j in 0..=20 {
                let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 20.0;
                let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0;
                let x = s.evaluate_point(k, Point2::new(u, v)).unwrap();
                let dist = |t: f64| x.distance(spine(t));
                let h = 2.0 * PI / f64::from(n);
                let mut best = 0;
                for m in 1..n {
                    if dist(f64::from(m) * h) < dist(f64::from(best) * h) {
                        best = m;
                    }
                }
                let (mut lo, mut hi) = (f64::from(best) * h - h, f64::from(best) * h + h);
                let g = 0.5 * (5f64.sqrt() - 1.0);
                for _ in 0..80 {
                    let (m1, m2) = (hi - g * (hi - lo), lo + g * (hi - lo));
                    if dist(m1) < dist(m2) {
                        hi = m2;
                    } else {
                        lo = m1;
                    }
                }
                worst = worst.max((dist(0.5 * (lo + hi)) - FILLET).abs());
            }
        }
    }
    worst
}

/// `(1/3) int (X - o).n dA` over the round's canal surface, `n` toward the ball's centre:
/// trapezoid over the closed spine (spectral), composite Simpson over the ball's arc from
/// contact `a` to contact `b`, the spine derivative by a fourth-order difference.
fn round_flux(ball: &dyn Fn(f64) -> Ball, o: Point3) -> (f64, f64) {
    let (nt, np) = (720, 256);
    let point = |t: f64, p: f64| {
        let bl = ball(t);
        let om = bl.a.dot(bl.b).acos();
        let e = (bl.a * ((1.0 - p) * om).sin() + bl.b * (p * om).sin()) * (1.0 / om.sin());
        let de = (bl.a * (-((1.0 - p) * om).cos()) + bl.b * (p * om).cos()) * (om / om.sin());
        (bl.c + e * FILLET, e, de * FILLET)
    };
    let h = 1e-3;
    let mut total = 0.0;
    let mut area = 0.0;
    for i in 0..nt {
        let t = 2.0 * PI * f64::from(i) / f64::from(nt);
        let mut inner = 0.0;
        let mut inner_area = 0.0;
        for j in 0..=np {
            let p = f64::from(j) / f64::from(np);
            let (x, e, xp) = point(t, p);
            let xt = (point(t - 2.0 * h, p).0 - point(t + 2.0 * h, p).0
                + (point(t + h, p).0 - point(t - h, p).0) * 8.0)
                * (1.0 / (12.0 * h));
            let da = xt.cross(xp).length();
            let wgt = if j == 0 || j == np {
                1.0
            } else if j % 2 == 1 {
                4.0
            } else {
                2.0
            };
            inner += wgt * (x - o).dot(-e) * da;
            inner_area += wgt * da;
        }
        total += inner / (3.0 * f64::from(np));
        area += inner_area / (3.0 * f64::from(np));
    }
    let dt = 2.0 * PI / f64::from(nt);
    (total * dt / 3.0, area * dt)
}

/// `(1/3) int (x - o).n dA` over the cylinder of radius `R` (axis through the origin,
/// radial frame `e1`, `e2`) between the edge and the rail, `n` into the cylinder: the rail
/// at the spine's height `ws` along the axis, the edge at `we`; trapezoid over the loop.
fn cylinder_flux(
    e1: Vec3,
    e2: Vec3,
    o: Point3,
    ws: &dyn Fn(f64) -> f64,
    we: &dyn Fn(f64) -> f64,
) -> f64 {
    let nt = 720;
    let mut total = 0.0;
    for i in 0..nt {
        let t = 2.0 * PI * f64::from(i) / f64::from(nt);
        let u = radial(e1, e2, t);
        total += -(R - o.dot(u)) * R * (ws(t) - we(t));
    }
    total * (2.0 * PI / f64::from(nt)) / 3.0
}

/// The plate (top face z = 0) and the r3 peg through the origin tilted 30 degrees from z,
/// from 10 below the plate's face to 15 above it along its axis, the rim rounded r1.
fn plate_and_peg(exact: bool) -> (Kernel, Brep, f64, Vec<(Surface, bool)>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let plate = create_box(&mut k, Point3::new(-20.0, -20.0, -20.0), 40.0, 40.0, 20.0).unwrap();
    let mut peg = create_cylinder(&mut k, Point3::new(0.0, 0.0, -10.0), R, 25.0).unwrap();
    tilt(&mut k, &mut peg, 30f64.to_radians());
    let mut b = boolean_union(&mut k, plate, peg).unwrap();
    let before = volume(&k, &mut b);
    let rim = edges_where(&k, &b, |m| {
        m.z.abs() < 1e-6 && m.x.abs() < 10.0 && m.y.abs() < 10.0
    });
    assert_eq!(rim.len(), 2, "the rim's two halves");
    fillet_edges(&mut k, &mut b, &rim, FILLET, 1, 1, 1.0).unwrap();
    let rounds = round_faces(&b);
    (k, b, before, rounds)
}

/// The peg's axis frame tilted by `tau`, its spine (the plane z = r through the r4 cylinder
/// about the axis) and the volume the round adds by the divergence theorem about the
/// origin: the plate's strip lies in z = 0 and contributes nothing, the cylinder's
/// `-(R / 3) R 2 pi r / cos(tau)`.
fn plate_and_peg_round(tau: f64) -> (impl Fn(f64) -> Point3, f64, f64) {
    let (e1, e2, a) = (
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, tau.cos(), tau.sin()),
        Vec3::new(0.0, -tau.sin(), tau.cos()),
    );
    // on the cylinder of radius rho about the peg's axis at angle t, the height along the
    // axis where the plane z = h is
    let w = move |rho: f64, h: f64, t: f64| (h - rho * radial(e1, e2, t).z) / a.z;
    let spine = move |t: f64| radial(e1, e2, t) * (R + FILLET) + a * w(R + FILLET, FILLET, t);
    let ball = |t: f64| Ball {
        c: spine(t),
        a: Vec3::new(0.0, 0.0, -1.0),
        b: -radial(e1, e2, t),
    };
    let o = Point3::new(0.0, 0.0, 0.0);
    let strip = cylinder_flux(e1, e2, o, &|t| w(R + FILLET, FILLET, t), &|t| w(R, 0.0, t));
    assert!((strip + R * R * 2.0 * PI * FILLET / (3.0 * tau.cos())).abs() < 1e-12);
    let (flux, area) = round_flux(&ball, o);
    (spine, flux + strip, area)
}

#[test]
fn the_quadrature_is_pappus_for_an_upright_peg() {
    // the cusp r^2 (1 - pi/4) with its centroid r (10 - 3 pi) / (12 - 3 pi) out from the peg
    let (_, added, _) = plate_and_peg_round(0.0);
    let xbar = FILLET * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let pappus = 2.0 * PI * (R + xbar) * FILLET * FILLET * (1.0 - PI / 4.0);
    assert!(
        (added - pappus).abs() < 1e-10 * pappus,
        "{added} != {pappus}"
    );
}

#[test]
fn an_oblique_plane_and_cylinder_round_is_the_exact_rational_ball_sweep() {
    // the plate and the peg's part above it (cut through its axis point: 15 of its length)
    let union = 40.0 * 40.0 * 20.0 + PI * R * R * 15.0;
    let (spine, added, area) = plate_and_peg_round(30f64.to_radians());

    let (k, mut b, before, rounds) = plate_and_peg(true);
    assert!((before - union).abs() < 1e-9 * union, "{before} != {union}");
    assert_eq!(rounds.len(), 2);
    assert!(
        rounds.iter().all(|(_, rational)| *rational),
        "the rounds are rational"
    );
    let off = off_spine(&k, &rounds, &spine);
    assert!(off < 1e-12, "the round is off the ball by {off:e}");
    let got = round_area(&k, &mut b);
    assert!(
        (got - area).abs() < 1e-9 * area,
        "round area {got} != {area}"
    );
    let after = volume(&k, &mut b);
    assert!(
        (after - before - added).abs() < VOLUME_TOL,
        "{before} + {added} != {after} (off {:e})",
        after - before - added
    );
}

#[test]
fn upstream_traces_the_oblique_plane_and_cylinder_round() {
    let (_, _, _, rounds) = plate_and_peg(false);
    assert_eq!(rounds.len(), 2);
    assert!(
        rounds.iter().all(|(_, rational)| !*rational),
        "the traced fit"
    );
}

/// The r3 pole up z from -12 to 8 and the 45 degree cone tilted 15 degrees, its apex
/// 3 / sin 45 from the origin, from 1 below the apex to 20 below it; the concave conic
/// where the cone leaves the pole rounded r1.
fn pole_and_cone(exact: bool) -> (Kernel, Brep, f64, Vec<(Surface, bool)>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let alpha = 45f64.to_radians();
    let d = R / alpha.sin();
    let pole = create_cylinder(&mut k, Point3::new(0.0, 0.0, -12.0), R, 20.0).unwrap();
    let mut skirt = create_cone(
        &mut k,
        Point3::new(0.0, 0.0, d - 20.0),
        20.0 * alpha.tan(),
        alpha.tan(),
        19.0,
    )
    .unwrap();
    tilt(&mut k, &mut skirt, 15f64.to_radians());
    let mut b = boolean_union(&mut k, pole, skirt).unwrap();
    let before = volume_to(&k, &mut b, 1e-10);
    // the conic's two edges (the pole's seam above it excluded); their curves are fitted
    let rim = edges_where(&k, &b, |m| {
        ((m.x * m.x + m.y * m.y).sqrt() - R).abs() < 1e-3 && m.z > -10.0 && m.z < 4.0
    });
    assert_eq!(rim.len(), 2, "the conic's two halves");
    fillet_edges(&mut k, &mut b, &rim, FILLET, 1, 1, 1.0).unwrap();
    let rounds = round_faces(&b);
    (k, b, before, rounds)
}

#[test]
fn two_quadrics_touching_one_ball_round_exactly_on_their_conic() {
    let (alpha, tau) = (45f64.to_radians(), 15f64.to_radians());
    let (e1, e2, a) = (
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    // toward the cone's apex from the ball's centre
    let uc = Vec3::new(0.0, -tau.sin(), tau.cos());
    let apex = uc * (R / alpha.sin());
    // A cylinder of radius rho about z and the cone tangent to the same ball of radius rho
    // about the origin meet where (x.a) cos(alpha) = +-(x.uc - rho sin(alpha)); the cone
    // leaves the pole on x.(a cos(alpha) + uc) = rho sin(alpha): on the cylinder at angle
    // t, the height
    let w = |rho: f64, t: f64| {
        rho * (alpha.sin() - radial(e1, e2, t).dot(uc)) / (alpha.cos() + tau.cos())
    };
    let rho = R + FILLET;
    let spine = |t: f64| radial(e1, e2, t) * rho + a * w(rho, t);
    let ball = |t: f64| {
        let c = spine(t);
        let across = (c - uc * c.dot(uc)).unitize().unwrap();
        Ball {
            c,
            a: -(across * alpha.cos() + uc * alpha.sin()),
            b: -radial(e1, e2, t),
        }
    };
    // about the apex the cone's strip contributes nothing
    let (flux, area) = round_flux(&ball, apex);
    let added = flux + cylinder_flux(e1, e2, apex, &|t| w(rho, t), &|t| w(R, t));

    let (k, mut b, before, rounds) = pole_and_cone(true);
    assert_eq!(rounds.len(), 2);
    assert!(
        rounds.iter().all(|(_, rational)| *rational),
        "the rounds are rational"
    );
    let off = off_spine(&k, &rounds, &spine);
    assert!(off < 1e-12, "the round is off the ball by {off:e}");
    let got = round_area(&k, &mut b);
    assert!(
        (got - area).abs() < 1e-9 * area,
        "round area {got} != {area}"
    );
    let after = volume_to(&k, &mut b, 1e-10);
    assert!(
        (after - before - added).abs() < VOLUME_TOL,
        "{before} + {added} != {after} (off {:e})",
        after - before - added
    );
}

#[test]
fn upstream_traces_the_round_between_two_quadrics_touching_one_ball() {
    let (_, _, _, rounds) = pole_and_cone(false);
    assert_eq!(rounds.len(), 2);
    assert!(
        rounds.iter().all(|(_, rational)| !*rational),
        "the traced fit"
    );
}
