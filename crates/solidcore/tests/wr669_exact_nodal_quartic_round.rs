//! WR-669 (`fixes.fillet_exact_round_classes`), the SolidCraft fork's tangency-labs/smlib@dda3b5d:
//! a round between two faces touching at one point the same way round is exact on the nodal
//! quartic their offsets meet in, where upstream (and the flag before that commit) traces it.
//!
//! Two pipes touching at a point: an r3 pipe up z, and an r1 pipe along +x from the plane
//! x = 1/2 out, its axis at y = 2, so that it lies inside the first's cylinder where they
//! touch at (0, 3, 0). The concave edge where the small pipe leaves the big one (an arc of
//! their figure-eight, ending on the small pipe's end disc short of the node) is rounded
//! r1/2. The ball's centre runs on the r3.5 and r1.5 offsets, which touch at (0, 3.5, 0):
//! the spine is their quartic's arc that keeps off that node.
//!
//! Every sampled point of the round lies at the radius from the closed-form spine and its
//! rails on their faces (to the fork's 1e-9, see [`OFF_BALL`]), and the round face's area
//! matches the canal surface's between the rails and the end disc's plane (Gauss-Legendre)
//! to [`AREA_TOL`]. Without the flag, or with the arm traced, the fillet fails.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3, PI};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId, FaceId};
use solidcore::{Brep, Kernel, Result};

/// The big pipe's radius (axis z).
const R: f64 = 3.0;
/// The small pipe's radius (axis along x at y = R - RHO, z = 0).
const RHO: f64 = 1.0;
/// Where the small pipe starts along x.
const X0: f64 = 0.5;
const FILLET: f64 = 0.5;
/// How far the round may be from the ball: the fork's exact-round criterion (1e-9 mm,
/// tangency-labs/smlib@95ce0c5). Its nodal-quartic formula in doubles loses about 1e-10
/// on the spine toward the trace's last point (W cancels by about 1e4 there: the second
/// point of a short line from the node); the round is 9e-10 off there, 1e-12 at the
/// first point. The traced round fails outright.
const OFF_BALL: f64 = 1e-9;
/// How far the round face's area may be from the canal surface's (relative): the face is
/// trimmed at the end disc by fitted UV curves (the end edges' tolerances are 6e-5; their
/// 3D curves lie on the disc's plane to 4e-11), which the area integrates: 3.8e-7 off.
/// The quadrature itself agrees with itself at double the panels and order to 1e-14.
const AREA_TOL: f64 = 1e-6;

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// Distance from the big pipe's cylinder.
fn off_a(x: Point3) -> f64 {
    ((x.x * x.x + x.y * x.y).sqrt() - R).abs()
}

/// Distance from the small pipe's cylinder.
fn off_b(x: Point3) -> f64 {
    (((x.y - (R - RHO)).powi(2) + x.z * x.z).sqrt() - RHO).abs()
}

/// The two pipes united and their concave edge rounded; the round faces.
fn pipes(exact: bool) -> (Kernel, Brep, Result<()>, Vec<FaceId>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let a = create_cylinder(&mut k, Point3::new(0.0, 0.0, -10.0), R, 20.0).unwrap();
    let mut small = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), RHO, 10.0 - X0).unwrap();
    // z to x: the small pipe from (X0, R - RHO, 0) along +x
    let pl = Axis2Placement::new(
        Point3::new(X0, R - RHO, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut small, &pl, None, false).unwrap();
    let mut b = boolean_union(&mut k, a, small).unwrap();
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    let rim: Vec<EdgeId> = edges
        .into_iter()
        .filter(|&e| {
            let m = midpoint(&k, &b, e);
            m.x > X0 + 1e-6 && off_a(m) < 1e-6 && off_b(m) < 1e-6
        })
        .collect();
    assert_eq!(rim.len(), 1, "the pipes' one concave edge");
    let res = fillet_edges(&mut k, &mut b, &rim, FILLET, 1, 1, 1.0);
    let rounds = b
        .faces()
        .ids()
        .filter(|&f| {
            matches!(
                face::surface(&b, f).unwrap().map(Surface::kind),
                Some(SurfaceKind::BSpline(_))
            )
        })
        .collect();
    (k, b, res, rounds)
}

/// The ball's centre: on the offsets (R + r about z, RHO + r about the small axis), at
/// angle `phi` round the small one from +y (the node at `phi = 0`), on the x > 0 lobe.
fn spine(phi: f64) -> Point3 {
    let y = (R - RHO) + (RHO + FILLET) * phi.cos();
    let z = (RHO + FILLET) * phi.sin();
    let x = ((R + FILLET).powi(2) - y * y).sqrt();
    Point3::new(x, y, z)
}

/// The angle where the centre's x is `x` (on (0, pi)).
fn phi_at_x(x: f64) -> f64 {
    let y = ((R + FILLET).powi(2) - x * x).sqrt();
    ((y - (R - RHO)) / (RHO + FILLET)).acos()
}

/// The point at `p` along the ball's arc at `phi`, from its contact with the big pipe
/// (p = 0) to the small one (p = 1), with its p derivative.
fn arc(phi: f64, p: f64) -> (Point3, Vec3) {
    let c = spine(phi);
    let a = -Vec3::new(c.x, c.y, 0.0).unitize().unwrap();
    let b = -Vec3::new(0.0, c.y - (R - RHO), c.z).unitize().unwrap();
    let om = a.dot(b).acos();
    let e = (a * ((1.0 - p) * om).sin() + b * (p * om).sin()) * (1.0 / om.sin());
    let de = (a * (-((1.0 - p) * om).cos()) + b * (p * om).cos()) * (om / om.sin());
    (c + e * FILLET, de * FILLET)
}

/// The n-point Gauss-Legendre nodes and weights on [-1, 1].
fn gauss(n: usize) -> Vec<(f64, f64)> {
    let legendre = |x: f64| {
        let (mut p0, mut p1) = (1.0, x);
        for k in 2..=n {
            let p2 = ((2 * k - 1) as f64 * x * p1 - (k - 1) as f64 * p0) / k as f64;
            p0 = p1;
            p1 = p2;
        }
        (p1, n as f64 * (x * p1 - p0) / (x * x - 1.0))
    };
    (0..n)
        .map(|i| {
            let mut x = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
            for _ in 0..100 {
                let (p, dp) = legendre(x);
                x -= p / dp;
            }
            let (_, dp) = legendre(x);
            (x, 2.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect()
}

/// `int f` over [a, b], composite `m` panels of Gauss-Legendre `g`.
fn integrate(g: &[(f64, f64)], a: f64, b: f64, m: usize, f: &dyn Fn(f64) -> f64) -> f64 {
    let h = (b - a) / m as f64;
    let mut s = 0.0;
    for i in 0..m {
        let (lo, hi) = (a + h * i as f64, a + h * (i + 1) as f64);
        for &(x, w) in g {
            s += w * 0.5 * (hi - lo) * f(0.5 * (lo + hi) + 0.5 * (hi - lo) * x);
        }
    }
    s
}

/// The canal surface's area where x >= X0, between the trace's ends (where the centre's
/// x is X0: beyond them the whole arc has x < X0, its contact with the small pipe at the
/// centre's x and the rest short of it). Up to `phi_a` (the big pipe's contact on x = X0)
/// the arc is cut at the p where x = X0; past it the whole arc counts.
fn canal_area() -> f64 {
    let g = gauss(20);
    let phi_e = phi_at_x(X0);
    // the big pipe's contact has x = cx R / (R + r)
    let phi_a = phi_at_x(X0 * (R + FILLET) / R);
    let h = 1e-3;
    let da = |phi: f64, p: f64| {
        let xt = (arc(phi - 2.0 * h, p).0 - arc(phi + 2.0 * h, p).0
            + (arc(phi + h, p).0 - arc(phi - h, p).0) * 8.0)
            * (1.0 / (12.0 * h));
        xt.cross(arc(phi, p).1).length()
    };
    // the p in [0, 1] where the arc crosses x = X0 (x rises to the centre's x at p = 1)
    let p_cut = |phi: f64| {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if arc(phi, mid).0.x < X0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let cut = |phi: f64| integrate(&g, p_cut(phi), 1.0, 4, &|p| da(phi, p));
    let whole = |phi: f64| integrate(&g, 0.0, 1.0, 4, &|p| da(phi, p));
    // by symmetry in z: twice the half from phi_e to pi
    2.0 * (integrate(&g, phi_e, phi_a, 8, &cut) + integrate(&g, phi_a, PI, 32, &whole))
}

/// The largest `| |X - spine| - r |` over a grid of the round's whole surface, the
/// distance to the spine minimised over its angle (a grid, then golden section); and the
/// rails' (v = min, max) largest distance from the nearer pipe.
fn off_ball(k: &Kernel, s: &Surface) -> (f64, f64) {
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let n = 4096;
    let (mut worst, mut rails) = (0.0f64, 0.0f64);
    for i in 0..=40 {
        for j in 0..=20 {
            let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 40.0;
            let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0;
            let x = s.evaluate_point(k, Point2::new(u, v)).unwrap();
            if j == 0 || j == 20 {
                rails = rails.max(off_a(x).min(off_b(x)));
            }
            let dist = |t: f64| x.distance(spine(t));
            let h = 2.0 * PI / f64::from(n);
            let mut best = 1;
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
    (worst, rails)
}

#[test]
fn the_quadrature_integrates_a_quarter_torus() {
    // the concave quarter round of r about a circle of radius rc (its centre line):
    // 2 pi int_0^(pi/2) (rc - r cos t) r dt = 2 pi r (rc pi / 2 - r)
    let g = gauss(20);
    let (rc, r) = (4.0, 0.5);
    let q = 2.0 * PI * integrate(&g, 0.0, 0.5 * PI, 4, &|t| (rc - r * t.cos()) * r);
    let expect = 2.0 * PI * r * (rc * 0.5 * PI - r);
    assert!((q - expect).abs() < 1e-13 * expect, "{q} != {expect}");
}

#[test]
fn pipes_touching_at_a_point_round_exactly_on_their_nodal_quartic() {
    let (k, mut b, res, rounds) = pipes(true);
    res.unwrap();
    assert_eq!(rounds.len(), 1, "one round face");
    let f = rounds[0];
    let s = face::surface(&b, f).unwrap().unwrap().clone();
    let SurfaceKind::BSpline(bs) = s.kind() else {
        panic!("a B-spline round");
    };
    assert!(bs.is_rational(), "the round is rational");
    // two cylinders: degrees (12, 2)
    assert_eq!(
        (bs.degree(SurfParamType::U), bs.degree(SurfParamType::V)),
        (12, 2)
    );
    let (off, rails) = off_ball(&k, &s);
    assert!(off < OFF_BALL, "the round is off the ball by {off:e}");
    assert!(rails < OFF_BALL, "a rail is off its face by {rails:e}");
    let area = canal_area();
    let got = solidcore::mass::integrator::properties::face_area(&k, &mut b, f, 1e-10).unwrap();
    assert!(
        (got - area).abs() < AREA_TOL * area,
        "round area {got} != {area} (off {:e})",
        got - area
    );
}

#[test]
fn upstream_traces_the_round_between_pipes_touching_at_a_point_and_fails() {
    let (_, _, res, _) = pipes(false);
    assert!(res.is_err(), "the traced round fails: {res:?}");
}
