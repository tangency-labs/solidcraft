//! The merge corner's closed form against what it has to be: on both rounds, tangent to both,
//! through the right ends, regular, inside each round's own face, and matching an independent
//! solve of the tangent in each section.

use super::*;
use truck_modeling::{BoundedCurve, ParametricCurve, ParametricSurface3D, SearchParameter};

use crate::body::from_p3;

fn v3(v: mt::Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// The first end of the wall on a plate (`curveblend_tests::wall_on_a_plate`): the wall's face
/// y = 2 (the wall below it), its end x = 0, the plate's side running on along (12, 8).
fn first_end() -> Faces {
    let s = Vec3::new(3.0, 2.0, 0.0) / 13f64.sqrt();
    Faces {
        v: Vec3::new(0.0, 2.0, 2.0),
        top: Vec3::Z,
        wall: Vec3::Y,
        side: Vec3::new(-2.0, 3.0, 0.0) / 13f64.sqrt(),
        end: -Vec3::X,
        along_wall: Vec3::X,
        along_side: s,
    }
}

/// The other end: the wall's face x = 20, its end y = -20, the side along (8, 16).
fn second_end() -> Faces {
    Faces {
        v: Vec3::new(20.0, -20.0, 2.0),
        top: Vec3::Z,
        wall: Vec3::X,
        side: Vec3::new(2.0, -1.0, 0.0) / 5f64.sqrt(),
        end: -Vec3::Y,
        along_wall: Vec3::Y,
        along_side: Vec3::new(1.0, 2.0, 0.0) / 5f64.sqrt(),
    }
}

/// A corner of angle `theta` turned to an oblique frame at `origin`, mirrored if `mirror` (the
/// end square to the wall).
fn turned(theta: f64, origin: Vec3, mirror: bool) -> Faces {
    // An oblique right-handed frame: x, y, z turned about (1, 2, 3) by 0.7.
    let k = Vec3::new(1.0, 2.0, 3.0) / 14f64.sqrt();
    let turn = |p: Vec3| {
        let (c, s) = (0.7f64.cos(), 0.7f64.sin());
        p * c + k.cross(p) * s + k * (k.dot(p) * (1.0 - c))
    };
    let y = if mirror { -1.0 } else { 1.0 };
    let local = |x: f64, yy: f64, z: f64| turn(Vec3::new(x, yy * y, z));
    Faces {
        v: origin,
        top: local(0.0, 0.0, 1.0),
        wall: local(0.0, 1.0, 0.0),
        side: local(-theta.sin(), theta.cos(), 0.0),
        end: local(-1.0, 0.0, 0.0),
        along_wall: local(1.0, 0.0, 0.0),
        along_side: local(theta.cos(), theta.sin(), 0.0),
    }
}

const RADII_TRIED: [(f64, f64); 6] = [(0.5, 0.5), (0.5, 0.25), (0.25, 0.5), (1.0, 0.5), (2.0, 0.5), (0.5, 2.0)];

/// Every zone the tests sweep: both ends of the analogue and turned corners of 5.5° to 110°,
/// mirrored and far from the origin, with each radius pair (those that pass the wall's end).
fn zones() -> Vec<Zone> {
    let mut frames = vec![first_end(), second_end()];
    for (i, deg) in [5.5, 10.0, 33.69, 60.0, 90.0, 110.0].into_iter().enumerate() {
        let origin = Vec3::new(1000.0 * i as f64, -3.0, 7.0);
        frames.push(turned(deg * std::f64::consts::PI / 180.0, origin, false));
        frames.push(turned(deg * std::f64::consts::PI / 180.0, origin, true));
    }
    let mut out = Vec::new();
    for f in &frames {
        for (ra, rb) in RADII_TRIED {
            if let Ok(z) = zone(f, ra, rb) {
                out.push(z);
            }
        }
    }
    assert!(out.len() > 60, "{}", out.len());
    out
}

/// Distance from `p` to the line through `a` along the unit `d`.
fn off_axis(p: Vec3, a: Vec3, d: Vec3) -> f64 {
    let q = p - a;
    (q - d * q.dot(d)).len()
}

/// The axes of `A` and `B`.
fn axes(z: &Zone) -> ((Vec3, Vec3), (Vec3, Vec3)) {
    ((z.v + z.m_w * z.ra + z.n * z.ra, z.w), (z.v + z.m_s * z.rb - z.n * z.rb, z.s))
}

/// The tolerance for points of a zone: relative to its size and its distance from the origin.
fn tol(z: &Zone) -> f64 {
    1e-12 * (z.r() + z.v.len())
}

fn samples() -> impl Iterator<Item = f64> {
    (0..=200).map(|i| i as f64 / 200.0)
}

#[test]
fn the_band_lies_on_both_rounds_and_each_ruling_touches_both() {
    for z in zones() {
        let ((a, da), (b, db)) = axes(&z);
        for t in samples() {
            let (ga, gb) = (z.gamma_a(t), z.gamma_b(t));
            assert!((off_axis(ga, a, da) - z.ra).abs() <= tol(&z), "{z:?} {t}");
            assert!((off_axis(gb, b, db) - z.rb).abs() <= tol(&z), "{z:?} {t}");
            // The ruling joins them and is square to each round's radius where it touches.
            assert!((gb - ga - z.ruling(t) * z.tau_b(t)).len() <= tol(&z), "{z:?} {t}");
            let d = z.ruling(t).normalized().unwrap();
            let radial = |p: Vec3, a: Vec3, da: Vec3| {
                let q = p - a;
                (q - da * q.dot(da)).normalized().unwrap()
            };
            // (The radius's direction carries the points' rounding, far from the origin.)
            assert!(radial(ga, a, da).dot(d).abs() <= tol(&z) / z.ra, "{z:?} {t}");
            assert!(radial(gb, b, db).dot(d).abs() <= tol(&z) / z.rb, "{z:?} {t}");
            // Each ruling lies in its section, square to the bisector.
            assert!(d.dot(z.b).abs() <= 1e-12);
            assert!(((ga - z.v).dot(z.b) - z.rho_star * (1.0 - t)).abs() <= tol(&z));
        }
    }
}

#[test]
fn the_nurbs_are_the_closed_form() {
    for z in zones() {
        let (ca, cb, patch) = (z.curve_a().unwrap(), z.curve_b().unwrap(), z.patch().unwrap());
        for t in samples() {
            assert!(from_p3(ca.subs(t)).dist(z.gamma_a(t)) <= tol(&z), "{z:?} {t}");
            assert!(from_p3(cb.subs(t)).dist(z.gamma_b(t)) <= tol(&z), "{z:?} {t}");
            for j in 0..=10 {
                let tau = z.r() / 2.0 * j as f64 / 10.0;
                assert!(from_p3(patch.subs(t, tau / z.r())).dist(z.point(t, tau)) <= tol(&z), "{z:?} {t} {tau}");
            }
            assert!(from_p3(patch.subs(t, z.tau_b(t) / z.r())).dist(z.gamma_b(t)) <= tol(&z), "{z:?} {t}");
        }
        // Degrees 3 and (4, 1), weights 1 + t² elevated: all positive.
        assert_eq!(ca.control_points().len(), 4);
        assert_eq!(patch.control_points().len(), 5);
        assert!(patch.control_points().iter().flatten().all(|c| c.w > 0.0));
    }
}

#[test]
fn the_band_runs_from_p_star_to_the_line_of_the_wall_end() {
    for z in zones() {
        let e = tol(&z);
        assert!(z.gamma_a(0.0).dist(z.p_star) <= e && z.gamma_b(0.0).dist(z.p_star) <= e, "{z:?}");
        assert!(z.gamma_a(1.0).dist(z.q_w) <= e && z.gamma_b(1.0).dist(z.q_s) <= e, "{z:?}");
        assert!(z.q_w.dist(z.v + z.n * z.ra) <= e && z.q_s.dist(z.v - z.n * z.rb) <= e);
        // P* is on T, rA from W and rB from S, ρ* along the bisector.
        let p = z.p_star - z.v;
        assert!(p.dot(z.n).abs() <= e && (p.dot(z.m_w) - z.ra).abs() <= e && (p.dot(z.m_s) - z.rb).abs() <= e);
        assert!((p.dot(z.b) - z.rho_star).abs() <= e);
        assert!((z.rho_star - z.r() / (2.0 * (z.theta / 2.0).sin())).abs() <= e);
        // The NURBS's ends and the patch's corners.
        let (ca, cb, patch) = (z.curve_a().unwrap(), z.curve_b().unwrap(), z.patch().unwrap());
        assert!(from_p3(ca.subs(0.0)).dist(z.p_star) <= e && from_p3(cb.subs(0.0)).dist(z.p_star) <= e);
        assert!(from_p3(ca.subs(1.0)).dist(z.q_w) <= e && from_p3(cb.subs(1.0)).dist(z.q_s) <= e);
        assert!(from_p3(patch.subs(0.0, 0.0)).dist(z.p_star) <= e);
        assert!(from_p3(patch.subs(1.0, 0.0)).dist(z.q_w) <= e && from_p3(patch.subs(1.0, 0.5)).dist(z.q_s) <= e);
        assert!(z.tau_b(0.0).abs() < 1e-300 && (z.tau_b(1.0) - z.r() / 2.0).abs() <= 1e-15 * z.r());
        // Along L at t = 1: the ruling runs straight down it.
        assert!((z.ruling(1.0) + z.n * 2.0).len() <= 1e-15);
        // The runs each round needs reach at least to P* along each trace.
        let (along_w, along_s) = z.needed_run();
        assert!(along_w >= p.dot(z.w) - e && along_s >= p.dot(z.s) - e, "{z:?}");
    }
}

#[test]
fn the_needed_run_is_as_far_as_each_edge_reaches_along_its_trace() {
    let mut frames = vec![first_end(), second_end()];
    for deg in [5.5, 33.69, 59.0, 61.0, 75.0, 90.0, 110.0, 140.0, 170.0] {
        frames.push(turned(f64::to_radians(deg), Vec3::new(4.0, -3.0, 7.0), false));
    }
    let radii = [(0.5, 0.5), (2.0, 0.5), (0.5, 2.0), (1.0, 0.1), (0.1, 1.0), (1.0, 0.75)];
    let mut past = 0;
    for z in frames.iter().flat_map(|f| radii.iter().filter_map(|(ra, rb)| zone(f, *ra, *rb).ok())) {
        // Densely sampled, then refined by a golden search on the best span.
        let reach = |along: &dyn Fn(f64) -> f64| {
            let n = 4000;
            let best = (0..=n).max_by(|i, j| along(*i as f64 / n as f64).total_cmp(&along(*j as f64 / n as f64))).unwrap();
            let (mut lo, mut hi) = ((best.max(1) - 1) as f64 / n as f64, ((best + 1).min(n)) as f64 / n as f64);
            let g = (5f64.sqrt() - 1.0) / 2.0;
            for _ in 0..80 {
                let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if along(a) < along(b) { lo = a } else { hi = b }
            }
            along(0.5 * (lo + hi)).max(along(0.0)).max(0.0)
        };
        let (along_w, along_s) = z.needed_run();
        let (w, s) = (reach(&|t| (z.gamma_a(t) - z.v).dot(z.w)), reach(&|t| (z.gamma_b(t) - z.v).dot(z.s)));
        let e = 1e-12 * (z.r() + z.v.len());
        assert!((along_w - w).abs() <= e && (along_s - s).abs() <= e, "{z:?}: {along_w} {along_s} against {w} {s}");
        if w > (z.p_star - z.v).dot(z.w) + 1e-6 || s > (z.p_star - z.v).dot(z.s) + 1e-6 {
            past += 1;
        }
    }
    // Some of the corners have an edge running on past P*.
    assert!(past >= 5, "{past}");
    // The review's cases: 90° with (2, 0.5), 75° with (2, 0.5), and 170° with (0.5, 2), where P* lies
    // behind the vertex along S's trace.
    let at = |deg: f64, ra: f64, rb: f64| zone(&turned(f64::to_radians(deg), Vec3::ZERO, false), ra, rb).unwrap().needed_run();
    let (w, s) = at(90.0, 2.0, 0.5);
    assert!((w - 0.879_311_103_710).abs() < 1e-9 && (s - 2.0).abs() < 1e-12, "{w} {s}");
    assert!((at(75.0, 2.0, 0.5).0 - 1.128_886_070_295).abs() < 1e-9);
    let (w, s) = at(170.0, 0.5, 2.0);
    assert!((w - 8.681_900_056_478).abs() < 1e-9 && (s - 3.087_152_267_019).abs() < 1e-9, "{w} {s}");
}

#[test]
fn the_patch_is_regular_with_no_pole_at_p_star() {
    for z in zones() {
        let patch = z.patch().unwrap();
        for i in 0..=40 {
            let t = i as f64 / 40.0;
            for j in 0..=8 {
                let tau = z.tau_b(t) * j as f64 / 8.0;
                let (du, dv) = (v3(patch.uder(t, tau / z.r())), v3(patch.vder(t, tau / z.r())) / z.r());
                // ∂X/∂t has b-component −ρ*, ∂X/∂τ is the ruling, square to b.
                assert!((du.dot(z.b) + z.rho_star).abs() <= 1e-9 * z.rho_star, "{z:?} {t} {tau}");
                assert!(dv.dist(z.ruling(t)) <= 1e-9 * z.r() && dv.dot(z.b).abs() <= 1e-9);
                // Never degenerate: ∂X/∂t's part along b alone gives |∂X/∂t × ∂X/∂τ| ≥ ρ*·|d̃|.
                assert!(du.cross(dv).len() >= 0.5 * z.rho_star * dv.len(), "{z:?} {t} {tau}");
            }
        }
    }
}

#[test]
fn each_edge_of_the_band_lies_inside_its_rounds_own_face() {
    for z in zones() {
        let e = tol(&z);
        for t in samples() {
            let (a, b) = (z.gamma_a(t) - z.v, z.gamma_b(t) - z.v);
            // A alone ends in S's plane; B alone ends in W's.
            assert!(a.dot(z.m_s) >= -e && b.dot(z.m_w) >= -e, "{z:?} {t}");
            // Each on its round's quarter turn between T and its wall.
            assert!(a.dot(z.n) >= -e && a.dot(z.n) <= z.ra + e && a.dot(z.m_w) >= -e && a.dot(z.m_w) <= z.ra + e, "{z:?} {t}");
            assert!(b.dot(z.n) <= e && b.dot(z.n) >= -z.rb - e && b.dot(z.m_s) >= -e && b.dot(z.m_s) <= z.rb + e, "{z:?} {t}");
            // Along the round's own run, not back past the vertex.
            assert!((a.dot(z.w) >= -e && b.dot(z.s) >= -e) || z.theta > std::f64::consts::FRAC_PI_2, "{z:?} {t}");
        }
    }
}

/// The section of the band at `ρ` solved on its own: the two rounds cut by the plane square to
/// the bisector are ellipses (semi-axes `r/c` across, `r` up); the band's profile is the tangent
/// line with `A` on one side and `B` on the other that is not `T`. Its normal `ν(φ)` is the root
/// of `f(φ) = ν·(C_B − C_A) − h_A(ν) − h_B(ν)` (`h` the ellipses' support functions) between
/// `φ = −π` (where `f = −w`) and `T`'s root at `−π/2`, found by a scan and safeguarded Newton.
/// Returns the touching points `(η, ζ)` across the section from `W`'s trace and up from `T`.
fn tangent_by_newton(theta: f64, ra: f64, rb: f64, rho: f64) -> ((f64, f64), (f64, f64)) {
    let c = (theta / 2.0).cos();
    let width = 2.0 * rho * (theta / 2.0).tan();
    let ((aa, ab), ca, cb) = ((ra / c, rb / c), (ra / c, ra), (width - rb / c, -rb));
    let support = |a: f64, r: f64, nu: (f64, f64)| (a * nu.0).hypot(r * nu.1);
    let f = |phi: f64| {
        let nu = (phi.cos(), phi.sin());
        nu.0 * (cb.0 - ca.0) + nu.1 * (cb.1 - ca.1) - support(aa, ra, nu) - support(ab, rb, nu)
    };
    let (mut lo, mut hi) = (-std::f64::consts::PI, f64::NAN);
    for i in 1..2000 {
        let phi = -std::f64::consts::PI + std::f64::consts::FRAC_PI_2 * i as f64 / 2000.0;
        if f(phi) > 0.0 {
            hi = phi;
            break;
        }
        lo = phi;
    }
    assert!(hi.is_finite(), "no sign change");
    let mut phi = 0.5 * (lo + hi);
    for _ in 0..100 {
        let h = 1e-7;
        let step = f(phi) / ((f(phi + h) - f(phi - h)) / (2.0 * h));
        let mut next = phi - step;
        if !(next > lo && next < hi) {
            next = 0.5 * (lo + hi);
        }
        if f(next) < 0.0 {
            lo = next;
        } else {
            hi = next;
        }
        phi = next;
        if hi - lo < 1e-15 {
            break;
        }
    }
    let nu = (phi.cos(), phi.sin());
    let (ha, hb) = (support(aa, ra, nu), support(ab, rb, nu));
    ((ca.0 + aa * aa * nu.0 / ha, ca.1 + ra * ra * nu.1 / ha), (cb.0 - ab * ab * nu.0 / hb, cb.1 - rb * rb * nu.1 / hb))
}

#[test]
fn each_section_matches_an_independent_solve_of_the_tangent() {
    for z in zones() {
        for i in 1..20 {
            let t = i as f64 / 20.0;
            let rho = z.rho_star * (1.0 - t);
            let ((ea, za), (eb, zb)) = tangent_by_newton(z.theta, z.ra, z.rb, rho);
            let o = z.v + z.w * (rho / (z.theta / 2.0).cos());
            let (pa, pb) = (o + z.lambda * ea + z.n * za, o + z.lambda * eb + z.n * zb);
            let e = 1e-8 * (z.r() + z.v.len());
            assert!(pa.dist(z.gamma_a(t)) <= e, "{z:?} {t}: {pa:?} {:?}", z.gamma_a(t));
            assert!(pb.dist(z.gamma_b(t)) <= e, "{z:?} {t}: {pb:?} {:?}", z.gamma_b(t));
        }
    }
}

#[test]
fn the_wall_on_a_plate_ends() {
    let deg = |x: f64| x * 180.0 / std::f64::consts::PI;
    let (ra, rb) = (1.0, 0.5);
    let first = zone(&first_end(), ra, rb).unwrap();
    let second = zone(&second_end(), ra, rb).unwrap();
    // tan θ0 = 2/3 and 1/2.
    assert!((deg(first.theta) - 33.690_067_525_979_8).abs() < 1e-9 && (first.theta.tan() - 2.0 / 3.0).abs() < 1e-12);
    assert!((deg(second.theta) - 26.565_051_177_077_99).abs() < 1e-9 && (second.theta.tan() - 0.5).abs() < 1e-12);
    // P* = (rB √13/2 + 1.5 rA, 2 + rA, 2) and (20 + rA, −20 + rB √5 + 2 rA, 2).
    assert!(first.p_star.dist(Vec3::new(rb * 13f64.sqrt() / 2.0 + 1.5 * ra, 2.0 + ra, 2.0)) < 1e-12);
    assert!(second.p_star.dist(Vec3::new(20.0 + ra, -20.0 + rb * 5f64.sqrt() + 2.0 * ra, 2.0)) < 1e-12);
    assert!(first.q_w.dist(Vec3::new(0.0, 2.0, 3.0)) < 1e-15 && first.q_s.dist(Vec3::new(0.0, 2.0, 1.5)) < 1e-15);
    // The design's runs: 2.401 and 2.553 at the first end, 3.118 and 3.236 at the other.
    let (w1, s1) = first.needed_run();
    let (w2, s2) = second.needed_run();
    assert!((w1 - 2.4014).abs() < 1e-4 && (s1 - 2.5528).abs() < 1e-4, "{w1} {s1}");
    assert!((w2 - 3.1180).abs() < 1e-4 && (s2 - 3.2361).abs() < 1e-4, "{w2} {s2}");
    // The ends are mirror images: the patch faces out at one and in at the other.
    assert_ne!(first.faces_out, second.faces_out);
}

#[test]
fn hostile_corners_are_refused() {
    let f = first_end();
    let nan = f64::NAN;
    let radii = [(nan, 0.5), (0.5, nan), (0.0, 0.5), (0.5, 0.0), (-1.0, 0.5), (0.5, -1.0), (1e300, 0.5), (0.5, f64::INFINITY), (1e-9, 0.5)];
    for (ra, rb) in radii {
        assert!(zone(&f, ra, rb).is_err(), "{ra} {rb}");
    }
    let bent = |g: &dyn Fn(&mut Faces)| {
        let mut f = first_end();
        g(&mut f);
        f
    };
    let bad = [
        bent(&|f| f.v = Vec3::new(nan, 0.0, 0.0)),
        bent(&|f| f.v = Vec3::new(1e300, 0.0, 0.0)),
        bent(&|f| f.top = Vec3::ZERO),
        bent(&|f| f.top = Vec3::new(0.0, 0.0, 1e-300)),
        bent(&|f| f.wall = Vec3::new(nan, 1.0, 0.0)),
        bent(&|f| f.end = Vec3::ZERO),
        bent(&|f| f.along_side = Vec3::new(f64::INFINITY, 0.0, 0.0)),
        // The wall's normal along T's: W lies in T.
        bent(&|f| f.wall = Vec3::Z),
        // The side leaning (a drafted side): L not square to T.
        bent(&|f| f.side = Vec3::new(-2.0, 3.0, 1.0)),
        // The wall's end leaning.
        bent(&|f| f.end = Vec3::new(-1.0, 0.0, 0.2)),
        // An edge that is not in its face.
        bent(&|f| f.along_wall = Vec3::new(1.0, 0.1, 0.0)),
        // W and S one plane: no corner.
        bent(&|f| {
            f.side = -Vec3::Y;
            f.along_side = Vec3::X;
        }),
        // T's corner reflex: the side's edge leaving the other way.
        bent(&|f| {
            f.along_side = -f.along_side;
            f.side = -f.side;
        }),
        // A corner of 3°.
        bent(&|f| {
            let a = 3f64.to_radians();
            f.along_side = Vec3::new(a.cos(), a.sin(), 0.0);
            f.side = Vec3::new(-a.sin(), a.cos(), 0.0);
        }),
        // The wall's end facing along the wall: P* would be past it.
        bent(&|f| f.end = Vec3::X),
    ];
    for f in bad {
        assert!(zone(&f, 0.5, 0.5).is_err(), "{f:?}");
    }
    // Tiny rounds far from the origin: the band's points would carry more rounding than the radius
    // allows. The same rounds near the origin are taken.
    let far = |d: f64, r: f64| zone(&turned(33.69f64.to_radians(), Vec3::new(d, d, d), false), r, r);
    assert!(far(1e9, 1e-6).is_err() && far(1e9, 1e-3).is_err() && far(1e6, 1e-6).is_err());
    assert!(far(1.0, 1e-6).is_ok() && far(1e3, 1e-3).is_ok() && far(1e9, 2.0).is_ok());
    // A corner so obtuse that the band reaches back past the wall's end (θ0 = 120°, rA cos θ0 + rB
    // = 0): refused; and a hair inside the tolerances is taken.
    assert!(zone(&turned(120f64.to_radians(), Vec3::ZERO, false), 0.5, 0.25).is_err());
    let nudged = bent(&|f| f.side += Vec3::new(0.0, 0.0, 1e-12));
    let (a, b) = (zone(&nudged, 0.5, 0.5).unwrap(), zone(&first_end(), 0.5, 0.5).unwrap());
    assert!(a.p_star.dist(b.p_star) < 1e-12);
}

/// Four-point Gauss–Legendre on `[−1, 1]`.
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// `∫ f` over `[a, b]`, four-point Gauss on each of `n` spans.
fn integrate(a: f64, b: f64, n: usize, f: impl Fn(f64) -> f64) -> f64 {
    let h = (b - a) / n as f64;
    (0..n).map(|i| GAUSS4.iter().map(|(x, w)| w * f(a + h * (i as f64 + 0.5 + 0.5 * x))).sum::<f64>() * h * 0.5).sum()
}

/// The volume the merge adds at one end beyond the two rounds alone (negative: takes away), per
/// section: the merged profile less each round's own, integrated across and along the bisector.
fn correction(z: &Zone) -> f64 {
    let c = (z.theta / 2.0).cos();
    let (aa, ab) = (z.ra / c, z.rb / c);
    // The ellipse of a round's section: its lower arc (`up`) or upper arc at `e` across.
    let arc = |e: f64, centre: (f64, f64), a: f64, r: f64, up: bool| {
        let x = (e - centre.0) / a;
        let h = r * (1.0 - x * x).max(0.0).sqrt();
        if up { centre.1 - h } else { centre.1 + h }
    };
    integrate(0.0, z.rho_star, 600, |rho| {
        let t = 1.0 - rho / z.rho_star;
        let width = 2.0 * rho * (z.theta / 2.0).tan();
        let o = z.v + z.w * (rho / c);
        let across = |p: Vec3| ((p - o).dot(z.lambda), (p - z.v).dot(z.n));
        let ((ea, za), (eb, zb)) = (across(z.gamma_a(t)), across(z.gamma_b(t)));
        let (ca, cb) = ((aa, z.ra), (width - ab, -z.rb));
        let merged = |e: f64| {
            if e <= ea {
                arc(e, ca, aa, z.ra, true)
            } else if e >= eb {
                arc(e, cb, ab, z.rb, false)
            } else {
                za + (zb - za) * (e - ea) / (eb - ea)
            }
        };
        let alone = |e: f64| {
            let a = if e < aa { arc(e, ca, aa, z.ra, true) } else { 0.0 };
            let b = if e > width - ab { arc(e, cb, ab, z.rb, false) } else { 0.0 };
            a + b
        };
        // Split at every kink so each span is smooth inside.
        let mut cuts = [0.0, ea, eb, aa.min(width), (width - ab).max(0.0), width];
        cuts.sort_by(f64::total_cmp);
        cuts.windows(2).map(|s| integrate(s[0], s[1], 8, |e| merged(e) - alone(e))).sum()
    })
}

#[test]
fn the_volume_the_merge_adds_matches_the_designs_table() {
    // (rA, rB) and the correction at the first end (θ0 33.69°) and the second (26.57°).
    let table = [
        ((0.5, 0.5), 0.0, 0.0),
        ((0.5, 0.25), -0.00606, -0.00751),
        ((1.0, 0.5), -0.04846, -0.06010),
        ((0.5, 1.0), 0.04846, 0.06010),
        ((2.0, 0.5), -0.3534, -0.4383),
    ];
    for ((ra, rb), first, second) in table {
        let c1 = correction(&zone(&first_end(), ra, rb).unwrap());
        let c2 = correction(&zone(&second_end(), ra, rb).unwrap());
        // The table's last digit.
        let digit = |x: f64| if x.abs() > 0.1 { 1e-4 } else { 1e-5 };
        assert!((c1 - first).abs() <= digit(first), "{ra} {rb}: {c1} against {first}");
        assert!((c2 - second).abs() <= digit(second), "{ra} {rb}: {c2} against {second}");
    }
    // Swapping the radii turns the band over: the same amount the other way.
    let swap = |f: Faces| correction(&zone(&f, 1.0, 0.5).unwrap()) + correction(&zone(&f, 0.5, 1.0).unwrap());
    assert!(swap(first_end()).abs() < 1e-9 && swap(second_end()).abs() < 1e-9);
}

#[test]
fn the_trimmed_band_is_a_closed_loop_round_the_outward_normal() {
    for z in zones() {
        let trimmed = z.trimmed().unwrap();
        let e = tol(&z);
        for (i, edge) in trimmed.edges.iter().enumerate() {
            let (from, to) = (trimmed.corners[i], trimmed.corners[(i + 1) % 3]);
            assert!(from_p3(edge.subs(0.0)).dist(from) <= e && from_p3(edge.subs(1.0)).dist(to) <= e, "{z:?} {i}");
        }
        assert!(trimmed.corners[0].dist(z.p_star) <= e);
        // The surface's normal points out: along A's outward normal on Γ_A (towards its axis) and
        // B's on Γ_B (away from its axis); the joins are tangent.
        let ((a, da), (b, db)) = axes(&z);
        let at = |t: f64, tau: f64| if z.faces_out { (t, tau / z.r()) } else { (tau / z.r(), t) };
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let out_a = {
                let q = a - z.gamma_a(t);
                (q - da * q.dot(da)) / z.ra
            };
            let out_b = {
                let q = z.gamma_b(t) - b;
                (q - db * q.dot(db)) / z.rb
            };
            let (u, v) = at(t, 0.0);
            let na = v3(trimmed.surface.normal(u, v));
            let (u, v) = at(t, z.tau_b(t));
            let nb = v3(trimmed.surface.normal(u, v));
            assert!(na.dist(out_a) <= 1e-9 && nb.dist(out_b) <= 1e-9, "{z:?} {t}: {na:?} {out_a:?} {nb:?} {out_b:?}");
        }
        // The loop winds counterclockwise seen from outside, about the middle of the band.
        let (u, v) = at(0.6, z.tau_b(0.6) / 2.0);
        let (mid, normal) = (from_p3(trimmed.surface.subs(u, v)), v3(trimmed.surface.normal(u, v)));
        let ring: Vec<Vec3> = trimmed.edges.iter().flat_map(|c| (0..100).map(move |k| from_p3(c.subs(k as f64 / 100.0)))).collect();
        let wound: f64 = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(p, q)| (*p - mid).cross(*q - mid).dot(normal)).sum();
        assert!(wound > 0.0, "{z:?}");
    }
}

fn tri(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    0.5 * (b - a).cross(c - a).len()
}

/// The band's area: `∫∫ |∂X/∂t × ∂X/∂τ|` over `0 ≤ τ ≤ τ_B(t)`, from the NURBS's own derivatives.
fn band_area(z: &Zone) -> f64 {
    let patch = z.patch().unwrap();
    integrate(0.0, 1.0, 64, |t| {
        integrate(0.0, z.tau_b(t), 4, |tau| v3(patch.uder(t, tau / z.r())).cross(v3(patch.vder(t, tau / z.r()))).len() / z.r())
    })
}

#[test]
fn the_trimmed_band_has_the_area_of_its_mesh() {
    for z in zones() {
        let area = band_area(&z);
        // Flat triangles between the rulings of the closed form, at two sizes, extrapolated: the
        // error of the twisted quads goes as the square of their size.
        let mesh = |n: usize, m: usize| {
            let row = |i: usize| -> Vec<Vec3> {
                let t = i as f64 / n as f64;
                (0..=m).map(|j| z.gamma_a(t).lerp(z.gamma_b(t), j as f64 / m as f64)).collect()
            };
            let mut sum = 0.0;
            let mut prev = row(0);
            for i in 1..=n {
                let next = row(i);
                for j in 0..m {
                    sum += tri(prev[j], next[j], next[j + 1]) + tri(prev[j], next[j + 1], prev[j + 1]);
                }
                prev = next;
            }
            sum
        };
        let (coarse, fine) = (mesh(400, 20), mesh(800, 40));
        let mesh = (4.0 * fine - coarse) / 3.0;
        assert!((area - mesh).abs() <= 1e-7 * area, "{z:?}: {area} against {mesh}");
        // A sanity bound: more than the flat triangle P*, q_W, q_S.
        assert!(area > tri(z.p_star, z.q_w, z.q_s));
    }
}

// ---------------------------------------------------------------------------------------------
// The band as a B-rep face.
// ---------------------------------------------------------------------------------------------

/// The sweep, and the analogue's ends at a hundredth and a hundred times its size.
fn band_zones() -> Vec<Zone> {
    let mut out = zones();
    for scale in [0.01, 100.0] {
        for f in [first_end(), second_end()] {
            let f = Faces { v: f.v * scale, ..f };
            for (ra, rb) in RADII_TRIED {
                out.push(zone(&f, ra * scale, rb * scale).unwrap());
            }
        }
    }
    out
}

/// A size for the zone's tolerances: its rounds and its distance from the origin.
fn size(z: &Zone) -> f64 {
    z.r() + z.v.len()
}

fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).len().atan2(a.dot(b))
}

/// `Γ_A′(t)`, differentiated by hand from the closed form.
fn gamma_a_dt(z: &Zone, t: f64) -> Vec3 {
    let (c, q) = ((z.theta / 2.0).cos(), 1.0 + t * t);
    -z.w * (z.rho_star / c) - z.lambda * (2.0 * z.ra * (1.0 - t * t) / (q * q * c)) + z.n * (4.0 * z.ra * t / (q * q))
}

/// `Γ_B′(t)`.
fn gamma_b_dt(z: &Zone, t: f64) -> Vec3 {
    let (c, q) = ((z.theta / 2.0).cos(), 1.0 + t * t);
    let across = -z.r() + 2.0 * z.rb * (1.0 - t * t) / (q * q);
    -z.w * (z.rho_star / c) + z.lambda * (across / c) - z.n * (4.0 * z.rb * t / (q * q))
}

/// `d̃′(t)`.
fn ruling_dt(z: &Zone, t: f64) -> Vec3 {
    -z.lambda * (2.0 * t / (z.theta / 2.0).cos()) - z.n * 2.0
}

/// `∂X/∂t × ∂X/∂τ` from the closed form.
fn closed_normal(z: &Zone, t: f64, tau: f64) -> Vec3 {
    (gamma_a_dt(z, t) + ruling_dt(z, t) * tau).cross(z.ruling(t))
}

/// Up out of the material in the section at `t`: the ruling turned a quarter towards `n` (the
/// material lies below the profile, which runs from `W` across to `S`).
fn section_up(z: &Zone, t: f64) -> Vec3 {
    z.lambda * (2.0 * t) + z.n * ((1.0 - t * t) / (z.theta / 2.0).cos())
}

/// The closed form's unit normal at `(t, τ)`, out of the material.
fn outward(z: &Zone, t: f64, tau: f64) -> Vec3 {
    let n = closed_normal(z, t, tau).normalized().unwrap();
    if n.dot(section_up(z, t)) < 0.0 { -n } else { n }
}

/// `(t, τ)` of a point of the band: its section along the bisector, then how far along that
/// section's ruling.
fn params(z: &Zone, p: Vec3) -> (f64, f64) {
    let t = 1.0 - (p - z.v).dot(z.b) / z.rho_star;
    let d = z.ruling(t);
    (t, (p - z.gamma_a(t)).dot(d) / d.dot(d))
}

/// The face's surface parameters at `(t, τ)`.
fn uv(z: &Zone, t: f64, tau: f64) -> (f64, f64) {
    if z.faces_out { (t, tau / z.r()) } else { (tau / z.r(), t) }
}

/// How far `p` lies above the merged profile in its own section, along `n` (below: inside the
/// material). The profile: `A`'s arc out to `Γ_A`, the ruling, `B`'s arc on from `Γ_B`.
fn above_profile(z: &Zone, p: Vec3) -> f64 {
    let c = (z.theta / 2.0).cos();
    let rho = (p - z.v).dot(z.b);
    let t = 1.0 - rho / z.rho_star;
    let o = z.v + z.w * (rho / c);
    let across = |q: Vec3| ((q - o).dot(z.lambda), (q - z.v).dot(z.n));
    let ((ea, za), (eb, zb), (e, h)) = (across(z.gamma_a(t)), across(z.gamma_b(t)), across(p));
    let width = 2.0 * rho * (z.theta / 2.0).tan();
    let (aa, ab) = (z.ra / c, z.rb / c);
    let profile = if e <= ea {
        z.ra - z.ra * (1.0 - ((e - aa) / aa).powi(2)).max(0.0).sqrt()
    } else if e >= eb {
        -z.rb + z.rb * (1.0 - ((e - (width - ab)) / ab).powi(2)).max(0.0).sqrt()
    } else {
        za + (zb - za) * (e - ea) / (eb - ea)
    };
    h - profile
}

fn band_of(z: &Zone) -> Band {
    let v = |p: Vec3| truck_modeling::builder::vertex(crate::body::p3(p));
    z.band(&v(z.p_star), &v(z.q_w), &v(z.q_s), 0.0).unwrap()
}

fn nurbs_of(face: &mt::Face) -> mt::NurbsSurface<mt::Vector4> {
    match face.oriented_surface() {
        mt::Surface::NurbsSurface(s) => s,
        s => panic!("not a NURBS: {s:?}"),
    }
}

/// The band's area from the closed form's own derivatives (not the NURBS's).
fn exact_area(z: &Zone) -> f64 {
    integrate(0.0, 1.0, 64, |t| integrate(0.0, z.tau_b(t), 4, |tau| closed_normal(z, t, tau).len()))
}

#[test]
fn the_derivatives_by_hand_are_the_closed_forms() {
    for z in band_zones() {
        let h = 1e-6;
        for i in 0..=20 {
            let t = 0.05 + 0.9 * i as f64 / 20.0;
            let diff = |f: &dyn Fn(f64) -> Vec3| (f(t + h) - f(t - h)) / (2.0 * h);
            let e = 1e-7 * size(&z);
            assert!(gamma_a_dt(&z, t).dist(diff(&|t| z.gamma_a(t))) <= e, "{z:?} {t}");
            assert!(gamma_b_dt(&z, t).dist(diff(&|t| z.gamma_b(t))) <= e, "{z:?} {t}");
            assert!(ruling_dt(&z, t).dist(diff(&|t| z.ruling(t))) <= 1e-7, "{z:?} {t}");
        }
    }
}

#[test]
fn the_band_is_one_face_on_its_three_edges() {
    for z in band_zones() {
        let band = band_of(&z);
        let e = tol(&z);
        let face = &band.face;
        assert!(face.orientation());
        // The edges by role: Γ_A and Γ_B from P*, L's piece from q_W down to q_S.
        let ends = |edge: &mt::Edge| (from_p3(edge.front().point()), from_p3(edge.back().point()));
        let at = |(a, b): (Vec3, Vec3), p: Vec3, q: Vec3| a.dist(p) <= e && b.dist(q) <= e;
        assert!(at(ends(&band.gamma_a), z.p_star, z.q_w) && at(ends(&band.gamma_b), z.p_star, z.q_s) && at(ends(&band.on_l), z.q_w, z.q_s));
        assert!(matches!(band.on_l.curve(), mt::Curve::Line(_)));
        assert!(band.gamma_a.front() == band.gamma_b.front() && band.gamma_a.back() == band.on_l.front() && band.on_l.back() == band.gamma_b.back());
        // One loop: Γ_A, L down and Γ_B back when the patch faces out; Γ_B, L up and Γ_A back otherwise.
        let loops = face.boundaries();
        assert_eq!(loops.len(), 1);
        let wire = &loops[0];
        let got: Vec<(mt::EdgeID, bool)> = wire.edge_iter().map(|x| (x.id(), x.orientation())).collect();
        let (a, l, b) = (band.gamma_a.id(), band.on_l.id(), band.gamma_b.id());
        let want = if z.faces_out { vec![(a, true), (l, true), (b, false)] } else { vec![(b, true), (l, false), (a, false)] };
        assert_eq!(got, want, "{z:?}");
        // Each edge's curve runs between its vertices, both ways round.
        for edge in wire.edge_iter() {
            let c = edge.oriented_curve();
            let (t0, t1) = c.range_tuple();
            assert!(from_p3(c.subs(t0)).dist(from_p3(edge.front().point())) <= e, "{z:?}");
            assert!(from_p3(c.subs(t1)).dist(from_p3(edge.back().point())) <= e, "{z:?}");
        }
        // A face on its own is an oriented shell, open along its three edges.
        let shell: mt::Shell = vec![face.clone()].into();
        assert_eq!(shell.shell_condition(), mt::ShellCondition::Oriented);
        // The corners: at q_W and q_S the face turns from L to the rounds' edges by
        // atan(k/rA) and atan(k/rB) (k = R/sin θ0); at P* from Γ_B to Γ_A, never a cusp.
        let edges: Vec<mt::Edge> = wire.edge_iter().cloned().collect();
        let k = z.r() / z.theta.sin();
        for i in 0..3 {
            let (into, out) = (&edges[(i + 2) % 3], &edges[i]);
            let (ci, co) = (into.oriented_curve(), out.oriented_curve());
            let corner = angle(-v3(ci.der(ci.range_tuple().1)), v3(co.der(co.range_tuple().0)));
            let p = from_p3(out.front().point());
            let want = if p.dist(z.q_w) <= e {
                (k / z.ra).atan()
            } else if p.dist(z.q_s) <= e {
                (k / z.rb).atan()
            } else {
                angle(gamma_a_dt(&z, 0.0), gamma_b_dt(&z, 0.0))
            };
            assert!((corner - want).abs() <= 1e-9 && corner > 1e-3, "{z:?} {i}: {corner} against {want}");
        }
    }
}

#[test]
fn the_bands_edges_lie_on_its_surface() {
    for z in band_zones() {
        let band = band_of(&z);
        let surface = nurbs_of(&band.face);
        // Well inside a part in 10⁹ of the size.
        let accept = 1e-11 * size(&z);
        for (which, edge) in [&band.gamma_a, &band.on_l, &band.gamma_b].into_iter().enumerate() {
            let c = edge.curve();
            let (t0, t1) = c.range_tuple();
            for i in 0..=64 {
                let x = c.subs(t0 + (t1 - t0) * i as f64 / 64.0);
                let p = from_p3(x);
                let (t, tau) = params(&z, p);
                let (u, v) = uv(&z, t, tau);
                assert!(from_p3(surface.subs(u, v)).dist(p) <= accept, "{z:?} {which} {i}");
                // On its own side of the trim: τ = 0, t = 1, τ = τ_B(t).
                let side = [tau, t - 1.0, tau - z.tau_b(t)][which];
                assert!(side.abs() <= accept && (-accept..=1.0 + accept).contains(&t), "{z:?} {which} {i}: {t} {tau}");
                // Truck's own search finds it there too.
                let (u, v) = surface.search_parameter(x, Some((u, v)), 100).unwrap();
                assert!(from_p3(surface.subs(u, v)).dist(p) <= accept, "{z:?} {which} {i}");
            }
        }
        // The planner's check of every boundary against its face.
        let solid = mt::Solid::new_unchecked(vec![vec![band.face.clone()].into()]);
        assert!(crate::tess::boundaries_on_surfaces(&solid, 1e-9 * size(&z)), "{z:?}");
    }
}

#[test]
fn the_bands_loop_is_simple_and_counterclockwise_in_its_parameters() {
    for z in band_zones() {
        let band = band_of(&z);
        let wire = &band.face.boundaries()[0];
        let ring: Vec<(f64, f64)> = wire
            .edge_iter()
            .flat_map(|edge| {
                let c = edge.oriented_curve();
                let (t0, t1) = c.range_tuple();
                (0..64).map(move |i| from_p3(c.subs(t0 + (t1 - t0) * i as f64 / 64.0))).collect::<Vec<_>>()
            })
            .map(|p| {
                let (t, tau) = params(&z, p);
                uv(&z, t, tau)
            })
            .collect();
        let n = ring.len();
        let seg = |i: usize| (ring[i], ring[(i + 1) % n]);
        let wound: f64 = (0..n).map(|i| seg(i).0.0 * seg(i).1.1 - seg(i).1.0 * seg(i).0.1).sum::<f64>() / 2.0;
        // The trim's area in (t, σ): ∫ t/(1 + t²) = ln 2 / 2, at any size.
        assert!((wound - 2f64.ln() / 2.0).abs() <= 1e-3, "{z:?}: {wound}");
        // No two segments cross but neighbours at their shared end.
        let cross = |(a, b): ((f64, f64), (f64, f64)), (c, d): ((f64, f64), (f64, f64))| {
            let side = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
            side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
        };
        for i in 0..n {
            for j in i + 2..n {
                if (j + 1) % n != i {
                    assert!(!cross(seg(i), seg(j)), "{z:?}: {i} {j}");
                }
            }
        }
    }
}

#[test]
fn the_bands_normals_point_out_square_to_its_rulings() {
    for z in band_zones() {
        let band = band_of(&z);
        let surface = nurbs_of(&band.face);
        let ((a, da), (b, db)) = axes(&z);
        for i in 0..=40 {
            let t = i as f64 / 40.0;
            for j in 0..=8 {
                let tau = z.tau_b(t) * j as f64 / 8.0;
                let (u, v) = uv(&z, t, tau);
                let normal = v3(surface.normal(u, v));
                // The tangent plane of the closed form: it holds the ruling and ∂X/∂t.
                assert!(normal.dot(z.ruling(t).normalized().unwrap()).abs() <= 1e-9, "{z:?} {t} {tau}");
                assert!(normal.dist(outward(&z, t, tau)) <= 1e-9, "{z:?} {t} {tau}");
                // Up out of the material in its section (at L, along it across to S's side).
                assert!(normal.dot(section_up(&z, t).normalized().unwrap()) > 0.0, "{z:?} {t} {tau}");
            }
            // On Γ_A it is A's normal (towards A's axis), on Γ_B it is B's (away from B's).
            let out_a = {
                let q = a - z.gamma_a(t);
                (q - da * q.dot(da)) / z.ra
            };
            let out_b = {
                let q = z.gamma_b(t) - b;
                (q - db * q.dot(db)) / z.rb
            };
            let (u, v) = uv(&z, t, 0.0);
            assert!(v3(surface.normal(u, v)).dist(out_a) <= 1e-9, "{z:?} {t}");
            let (u, v) = uv(&z, t, z.tau_b(t));
            assert!(v3(surface.normal(u, v)).dist(out_b) <= 1e-9, "{z:?} {t}");
        }
        // Up T's normal at P*, square to n along L.
        let (u, v) = uv(&z, 0.0, 0.0);
        assert!(v3(surface.normal(u, v)).dist(z.n) <= 1e-9);
        for j in 0..=8 {
            let (u, v) = uv(&z, 1.0, z.r() / 2.0 * j as f64 / 8.0);
            assert!(v3(surface.normal(u, v)).dot(z.n).abs() <= 1e-9);
        }
        // A step off the band along its normal leaves the material; a step against it enters.
        let step = 1e-5 * z.r();
        for i in 1..10 {
            let t = i as f64 / 10.0;
            for j in 1..10 {
                let tau = z.tau_b(t) * j as f64 / 10.0;
                let (u, v) = uv(&z, t, tau);
                let (p, normal) = (from_p3(surface.subs(u, v)), v3(surface.normal(u, v)));
                assert!(above_profile(&z, p).abs() <= 1e-9 * size(&z), "{z:?} {t} {tau}");
                assert!(above_profile(&z, p + normal * step) > 0.0 && above_profile(&z, p - normal * step) < 0.0, "{z:?} {t} {tau}");
            }
        }
    }
}

/// A mesh of the band, measured.
#[derive(Debug)]
struct Meshed {
    /// The triangles' own area.
    flat: f64,
    /// Their signed area in the face's parameters, and its size: what they tile of the trim.
    tiled: f64,
    tiled_abs: f64,
    /// Triangles with two corners at one point (within 1e-11 of the size), which have no
    /// direction, and triangles wound against the outward normal, with their area.
    repeated: usize,
    inverted: usize,
    inverted_area: f64,
}

/// Measures a mesh of the band, checking every corner lies on the band's surface and inside its
/// trim. Truck's mesher puts the boundary's points on the surface by its own search, which settles
/// within about 1e-8 along it: the trim is held to a millionth of the band.
fn checked_mesh(z: &Zone, tris: &[[Vec3; 3]], how: &str) -> Meshed {
    assert!(!tris.is_empty(), "{how} {z:?}");
    let (on, inside) = (1e-9 * size(z), 1e-6 * z.r());
    let mut m = Meshed { flat: 0.0, tiled: 0.0, tiled_abs: 0.0, repeated: 0, inverted: 0, inverted_area: 0.0 };
    for [a, b, c] in tris {
        let mut corners = [(0.0, 0.0); 3];
        for (k, p) in [a, b, c].into_iter().enumerate() {
            let (t, tau) = params(z, *p);
            assert!(z.point(t, tau).dist(*p) <= on, "{how} {z:?}: {p:?} off the band");
            let trim = (-t).max(t - 1.0).max(-tau / z.r()).max((tau - z.tau_b(t)) / z.r());
            assert!(trim <= inside / z.r(), "{how} {z:?}: {p:?} outside the trim at {t} {tau}");
            corners[k] = uv(z, t, tau);
        }
        let [(u0, v0), (u1, v1), (u2, v2)] = corners;
        let tiled = 0.5 * ((u1 - u0) * (v2 - v0) - (u2 - u0) * (v1 - v0));
        m.tiled += tiled;
        m.tiled_abs += tiled.abs();
        let cross = (*b - *a).cross(*c - *a);
        m.flat += 0.5 * cross.len();
        if a.dist(*b).min(b.dist(*c)).min(c.dist(*a)) <= 1e-11 * size(z) {
            m.repeated += 1;
            continue;
        }
        let (t, tau) = params(z, (*a + *b + *c) / 3.0);
        let t = t.clamp(0.0, 1.0);
        if cross.dot(outward(z, t, tau.clamp(0.0, z.tau_b(t)))) <= 0.0 {
            m.inverted += 1;
            m.inverted_area += 0.5 * cross.len();
        }
    }
    m
}

/// Truck's mesher takes no chord under 1e-6.
fn chord_for(z: &Zone) -> f64 {
    (1e-4 * z.r()).max(2e-6)
}

fn reference_mesh(z: &Zone, face: &mt::Face, chord: f64) -> Vec<[Vec3; 3]> {
    use truck_meshalgo::prelude::*;
    let shell: mt::Shell = vec![face.clone()].into();
    let meshed = shell.robust_triangulation_within(Deflection { display: true, ..Deflection::chord(chord) });
    let pm = meshed.face_iter().next().and_then(|f| f.surface()).unwrap_or_else(|| panic!("{z:?}: not meshed"));
    let pos = pm.positions();
    pm.faces().triangle_iter().map(|t| [0, 1, 2].map(|k| from_p3(pos[t[k].pos]))).collect()
}

#[test]
fn the_band_meshes_to_its_area_both_ways() {
    for z in band_zones() {
        let band = band_of(&z);
        let exact = exact_area(&z);
        // The closed form's area against the NURBS's own derivatives.
        assert!((exact - band_area(&z)).abs() <= 1e-10 * exact, "{z:?}");
        assert!(exact > tri(z.p_star, z.q_w, z.q_s));
        let chord = chord_for(&z);
        // A flat facet sagging `chord` from a surface of curvature 1/r misses about (2/3)·chord/r of
        // its area; the smaller radius bounds the curvature.
        let bound = 2.0 * chord / z.ra.min(z.rb) * exact;
        // The trim's area in (t, σ), which the triangles must tile once over: ln 2 / 2, less the
        // chords along Γ_B.
        let trim = 2f64.ln() / 2.0;
        let tiles = |m: &Meshed, how: &str| {
            assert!((m.tiled - trim).abs() <= 1e-3 * trim, "{how} {z:?}: tiles {} of {trim}", m.tiled);
            assert!(m.tiled_abs - m.tiled <= 1e-6 * trim, "{how} {z:?}: folded {m:?}");
        };
        // As the body meshes (truck's mesher, ruling lines on): every triangle wound out. Its
        // facets reach across the band's twist, so their area runs over by up to a few parts in
        // 10³ at wide corners: then a finer mesh comes closer.
        let reference = checked_mesh(&z, &reference_mesh(&z, &band.face, chord), "reference");
        tiles(&reference, "reference");
        assert!(reference.inverted == 0 && reference.repeated == 0, "{z:?}: {reference:?}");
        let over = (reference.flat - exact).abs();
        assert!(over <= 4e-3 * exact, "{z:?}: meshed {} against {exact}", reference.flat);
        if over > bound {
            let finer = checked_mesh(&z, &reference_mesh(&z, &band.face, chord / 16.0), "finer");
            assert!((finer.flat - exact).abs() <= over / 3.0, "{z:?}: {} then {} against {exact}", reference.flat, finer.flat);
        }
        // As the display plan meshes. At the narrowest corner (5.5°, with one radius four times
        // the other) it falls back to Delaunay and leaves triangles on repeated points along Γ_B;
        // on the smallest parts a sliver may lie a hair the wrong way. Neither covers anything.
        let solid = mt::Solid::new_unchecked(vec![vec![band.face.clone()].into()]);
        let (plan, stats) = crate::tess::plan(&solid, chord, None, None).unwrap();
        assert_eq!(stats.planar, 0);
        let mesh = plan.evaluate();
        let tris: Vec<[Vec3; 3]> = mesh.triangles.iter().map(|t| t.map(|i| mesh.positions[i as usize])).collect();
        let display = checked_mesh(&z, &tris, "display");
        tiles(&display, "display");
        assert!(display.repeated == 0 || z.theta < 6f64.to_radians(), "{z:?}: {display:?}");
        assert!(display.inverted_area <= 1e-6 * exact, "{z:?}: {display:?}");
        assert!((display.flat - exact).abs() <= bound, "{z:?}: planned {} against {exact}", display.flat);
        // At the analogue's own size, both meshes are clean.
        if z.r() >= 0.5 && z.v.len() < 50.0 && z.theta > 6f64.to_radians() {
            assert!(display.inverted == 0 && display.repeated == 0, "{z:?}: {display:?}");
        }
    }
}

#[test]
fn a_band_on_misplaced_corners_is_refused() {
    let z = zone(&first_end(), 1.0, 0.5).unwrap();
    let v = |p: Vec3| truck_modeling::builder::vertex(crate::body::p3(p));
    let (p, qw, qs) = (v(z.p_star), v(z.q_w), v(z.q_s));
    assert!(z.band(&p, &qw, &qs, 0.0).is_ok());
    // Each corner off by 1e-6, taken within a tolerance of 1e-5 and refused within 1e-7.
    let off = Vec3::new(0.0, 0.0, 1e-6);
    let nudged = [(v(z.p_star + off), qw.clone(), qs.clone()), (p.clone(), v(z.q_w + off), qs.clone()), (p.clone(), qw.clone(), v(z.q_s + off))];
    for (a, b, c) in &nudged {
        assert!(z.band(a, b, c, 1e-5).is_ok() && z.band(a, b, c, 1e-7).is_err());
    }
    // The corners in the wrong roles, one vertex twice, and hostile tolerances.
    assert!(z.band(&qw, &p, &qs, 1e-3).is_err() && z.band(&p, &qs, &qw, 1e-3).is_err());
    assert!(z.band(&p, &qw, &qw, 1e-3).is_err());
    for tol in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(z.band(&p, &qw, &qs, tol).is_err(), "{tol}");
    }
}
