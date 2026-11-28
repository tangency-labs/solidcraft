//! #18490 review: the UV trims the sphere drop makes (`Surface::drop_curve_base`, with the
//! subdivision of spans proved off the curve) given a certified bound, piece by piece, of
//! their image's distance from the edge they trim, rather than a sampled one.
//!
//! A trim is the drop's cubic Hermite spans; each piece of it (a span, split at the edge
//! curve's knots) is a cubic polynomial in UV, its image on the sphere `S(uv(t))`. The
//! certificate of `surface_intersector::sphere_span::sphere_span_image` (the one
//! `span_proved_off` asks) bounds `|S(uv(t)) - X(t)|` over the whole piece for a 3D cubic
//! `X` (PROVEN there: Peano kernel bound of the net's derivatives, outward rounding):
//! - an edge that is a non-rational cubic B-spline (the sphere/plane circle of
//!   `Fillet/sphere_plane.smb`): `X` is the edge's own cubic piece, on the same parameter
//!   (`create_uv_trim_curve` keeps the two ranges one), so the bound is the image's
//!   distance from the edge at each parameter;
//! - an edge on a circle (an analytic circle; or a circle's NURB, rational quadratic with
//!   double interior knots, whose own distance from the circle through three of its points
//!   is bounded the same way and asserted at most `1e-9` of the radius): `X` is the Hermite
//!   of the image's own ends (`S(uv)`, `S_u u' + S_v v'`), halved until within a quarter of
//!   the tolerance of the circle, and `X`'s distance from the circle is bounded from its
//!   Bezier control points: with `h(t)` its height over the circle's plane and
//!   `q(t) = |X - c|^2 - h^2 - rho^2`, both polynomials, their Bernstein coefficients bound
//!   them (convex hull, the curve in 32 de Casteljau pieces), and a point's distance from the
//!   circle is `sqrt(h^2 + (r - rho)^2)` with `|r - rho| = |q| / (r + rho) <= |q| / rho`.
//!   The coefficients are computed in floating point and widened by `1e-12` of their scale,
//!   far above their rounding (ARGUED). The certificate is asked within what is left of the
//!   tolerance, and the sum is the bound.
//!
//! Asserted within the edge's tolerance for every piece of: the regression circle 3e-5 from
//! a unit sphere's poles (`boolean_api::a_circle_passing_near_a_spheres_poles_gets_a_trim_curve_on_the_circle`,
//! 104 pieces, within 3.8e-6 of 1e-5; its first span was 5e-4 off before the change), and
//! the drop trims of `Fillet/sphere_plane.smb` and `Fillet/sphere_diff_3cylinders.smb`, the
//! files of the `own_reader_sphere_plane_*` / `own_reader_sphere_diff_3cylinders_*` goldens
//! and the `stubs4` rounds the change moves (`sphere_diff_3cylinders`: 30, 60 and 120 pieces,
//! within 4.2e-6 of 1e-5; `sphere_plane`'s circle within 1.84e-5 of its 1.85e-5: that edge
//! lies off the sphere by nearly its tolerance, and the image lies on the sphere; its seam
//! and second circle 45 pieces each, within 4.0e-6 of 1e-5).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr
)] // test helpers

use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::vector2d::Point2;
use solidcore::intersect::surface_intersector::sphere_span::{sphere_span_image, SpanImage};
use solidcore::math::det;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives as api;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p3::create_uv_trim_curve;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, edgeuse, face, EdgeuseId};
use solidcore::{Brep, Kernel};

/// A circle: centre, unit normal, radius.
struct Circle {
    centre: Point3,
    normal: Vec3,
    radius: f64,
}

/// Binomial coefficients of degree 3 and 6.
const C3: [f64; 4] = [1.0, 3.0, 3.0, 1.0];
const C6: [f64; 7] = [1.0, 6.0, 15.0, 20.0, 15.0, 6.0, 1.0];

/// The Bernstein coefficients of the product of two cubics given by theirs.
fn product(a: [f64; 4], b: [f64; 4]) -> [f64; 7] {
    let mut out = [0.0; 7];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += C3[i] * C3[j] / C6[i + j] * ai * bj;
        }
    }
    out
}

/// The control points of the piece `[a, b]` of the cubic Bezier curve `p` (`0 <= a < b <=
/// 1`), by de Casteljau.
fn sub_bezier(p: [Vec3; 4], a: f64, b: f64) -> [Vec3; 4] {
    let split = |q: [Vec3; 4], t: f64| {
        let l = |x: Vec3, y: Vec3| x + (y - x) * t;
        let (q01, q12, q23) = (l(q[0], q[1]), l(q[1], q[2]), l(q[2], q[3]));
        let (q012, q123) = (l(q01, q12), l(q12, q23));
        let mid = l(q012, q123);
        ([q[0], q01, q012, mid], [mid, q123, q23, q[3]])
    };
    let right = if a > 0.0 { split(p, a).1 } else { p };
    if b < 1.0 {
        split(right, (b - a) / (1.0 - a)).0
    } else {
        right
    }
}

/// The pieces [`bezier_off_circle`] bounds one by one: the convex-hull bound of a piece
/// of length `w` is within `O(w^2)` of the true maximum.
const PIECES: u32 = 32;

/// A bound of the distance from `circle` of the cubic Bezier curve with control points
/// `b`: the largest bound of its [`PIECES`] equal pieces. A piece's control points are
/// those of the curve over its parameter range (de Casteljau, rounded: within the margin).
fn bezier_off_circle(b: [Vec3; 4], circle: &Circle) -> f64 {
    let n = f64::from(PIECES);
    (0..PIECES)
        .map(|i| {
            let piece = sub_bezier(b, f64::from(i) / n, f64::from(i + 1) / n);
            piece_off_circle(piece, circle)
        })
        .fold(0.0, f64::max)
}

/// A bound of the distance from `circle` of one cubic Bezier piece (module docs).
fn piece_off_circle(b: [Vec3; 4], circle: &Circle) -> f64 {
    let w = b.map(|p| p - circle.centre);
    let h = w.map(|v| v.dot(circle.normal));
    let x = w.map(|v| v.x);
    let y = w.map(|v| v.y);
    let z = w.map(|v| v.z);
    let rho2 = circle.radius * circle.radius;
    let (xx, yy, zz, hh) = (product(x, x), product(y, y), product(z, z), product(h, h));
    let scale = w.iter().map(|v| v.dot(*v)).fold(rho2, f64::max);
    let margin = 1e-12 * scale;
    let q_max = (0..7)
        .map(|k| (xx[k] + yy[k] + zz[k] - hh[k] - rho2).abs())
        .fold(0.0, f64::max)
        + margin;
    let h_max = h.iter().map(|v| v.abs()).fold(0.0, f64::max) + 1e-12 * scale.sqrt();
    (h_max * h_max + det::powi(q_max / circle.radius, 2)).sqrt()
}

/// What a trim is certified against: the circle an analytic circle edge or a circle's
/// NURB (rational quadratic, double interior knots) lies on, with a bound of the edge's
/// distance from it (0 for the analytic circle); or the edge's own curve where that is a
/// non-rational cubic (or lower) B-spline.
enum Reference<'a> {
    Circle(Circle, f64),
    Cubic(&'a Curve),
}

/// The reference of `curve` (`None` for any other kind).
fn reference_of(curve: &Curve) -> Option<Reference<'_>> {
    match curve.kind() {
        CurveKind::Ellipse(ellipse) => {
            let c = ellipse.canonical();
            ((c.radius_x - c.radius_y).abs() <= 1e-12 * c.radius_x).then(|| {
                let z = c.position.z_axis();
                let circle = Circle {
                    centre: c.position.origin(),
                    normal: z * (1.0 / z.length()),
                    radius: c.radius_x,
                };
                Reference::Circle(circle, 0.0)
            })
        }
        CurveKind::BSpline(s) if !s.is_rational().unwrap() && s.degree().unwrap() <= 3 => {
            Some(Reference::Cubic(curve))
        }
        CurveKind::BSpline(s) if s.degree().unwrap() == 2 => arc_segments(curve).map(|segments| {
            let circle = circle_of_arcs(&segments);
            let off = arcs_off_circle(&segments, &circle);
            Reference::Circle(circle, off)
        }),
        _ => None,
    }
}

/// The rational quadratic Bezier segments of a B-spline whose interior knots are all
/// double (the form of a circle's NURB): each as its three homogeneous control points
/// `(w x, w y, w z)` and weights. `None` for any other knot structure.
fn arc_segments(curve: &Curve) -> Option<Vec<([Vec3; 3], [f64; 3])>> {
    let CurveKind::BSpline(s) = curve.kind() else {
        return None;
    };
    let (_, mults) = s.knots(None).unwrap();
    let n = mults.len();
    let interior_double = mults
        .iter()
        .skip(1)
        .take(n.saturating_sub(2))
        .all(|&m| m == 2);
    let cps = s.control_points().unwrap();
    if n < 2
        || mults[0] != 3
        || mults[n - 1] != 3
        || !interior_double
        || cps.len() != 2 * (n - 1) + 1
    {
        return None;
    }
    Some(
        (0..n - 1)
            .map(|i| {
                let c = &cps[2 * i..2 * i + 3];
                (
                    [0, 1, 2].map(|j| Vec3::new(c[j].x, c[j].y, c[j].z)),
                    [0, 1, 2].map(|j| c[j].w),
                )
            })
            .collect(),
    )
}

/// The circle through three points spread along `segments` (their ends and middles).
fn circle_of_arcs(segments: &[([Vec3; 3], [f64; 3])]) -> Circle {
    let mut on: Vec<Vec3> = Vec::new();
    for (p, w) in segments {
        on.push(p[0] * (1.0 / w[0]));
        on.push((p[0] + p[1] * 2.0 + p[2]) * (1.0 / (w[0] + 2.0 * w[1] + w[2])));
    }
    let (last_p, last_w) = segments[segments.len() - 1];
    on.push(last_p[2] * (1.0 / last_w[2]));
    let n = on.len();
    let (a, b, c) = (on[0], on[n / 3], on[2 * n / 3]);
    // the circumcentre of a, b, c
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let centre = a
        + (normal.cross(ab) * ac.dot(ac) + ac.cross(normal) * ab.dot(ab))
            * (1.0 / (2.0 * normal.dot(normal)));
    Circle {
        centre,
        normal: normal * (1.0 / normal.length()),
        radius: (a - centre).length(),
    }
}

/// Binomial coefficients of degree 2 and 4.
const C2: [f64; 3] = [1.0, 2.0, 1.0];
const C4: [f64; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];

/// The Bernstein coefficients of the product of two quadratics given by theirs.
fn product2(a: [f64; 3], b: [f64; 3]) -> [f64; 5] {
    let mut out = [0.0; 5];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += C2[i] * C2[j] / C4[i + j] * ai * bj;
        }
    }
    out
}

/// A bound of the distance of the rational quadratic `segments` from `circle`: with the
/// homogeneous numerator `N` and weight `W`, the height `h = n . (N - c W)` (quadratic) and
/// `q = |N - c W|^2 - h^2 - rho^2 W^2` (quartic) bound the point's height `h / W` and
/// `|r - rho| <= |q| / (W^2 rho)` by their Bernstein coefficients and the least weight
/// coefficient (positive weights, convex hull), widened by `1e-12` of their scale.
fn arcs_off_circle(segments: &[([Vec3; 3], [f64; 3])], circle: &Circle) -> f64 {
    let mut worst: f64 = 0.0;
    for (p, w) in segments {
        let v = [0, 1, 2].map(|i| p[i] - circle.centre * w[i]);
        let h = v.map(|x| x.dot(circle.normal));
        let (x, y, z) = (v.map(|a| a.x), v.map(|a| a.y), v.map(|a| a.z));
        let rho2 = circle.radius * circle.radius;
        let (xx, yy, zz, hh, ww) = (
            product2(x, x),
            product2(y, y),
            product2(z, z),
            product2(h, h),
            product2(*w, *w),
        );
        let scale = v
            .iter()
            .map(|a| a.dot(*a))
            .fold(rho2 * ww.iter().fold(0.0, |m, &x| f64::max(m, x)), f64::max);
        let margin = 1e-12 * scale;
        let q_max = (0..5)
            .map(|k| (xx[k] + yy[k] + zz[k] - hh[k] - rho2 * ww[k]).abs())
            .fold(0.0, f64::max)
            + margin;
        let h_max = h.iter().map(|a| a.abs()).fold(0.0, f64::max) + 1e-12 * scale.sqrt();
        let w_min = w.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(w_min > 0.0, "a weight that is not positive");
        let d = (det::powi(h_max / w_min, 2)
            + det::powi(q_max / (w_min * w_min * circle.radius), 2))
        .sqrt();
        worst = worst.max(d);
    }
    worst
}

/// The distinct knots of a B-spline curve (none for any other kind).
fn breaks(curve: &Curve) -> Vec<f64> {
    match curve.kind() {
        CurveKind::BSpline(s) => s.knots(None).unwrap().0,
        _ => Vec::new(),
    }
}

/// The cubic Hermite data `[p0, d0, p1, d1]` of `curve` over `[t0, t1]` (derivatives
/// scaled to a `[0, 1]` parameter), each end from inside the interval (`from_left`: the
/// span starting there), checked to reproduce the curve at the middle: the piece is one
/// cubic polynomial there.
fn hermite(k: &Kernel, curve: &Curve, t0: f64, t1: f64) -> [Vec3; 4] {
    let dt = t1 - t0;
    let start = curve.evaluate(k, t0, 1, true, false).unwrap();
    let end = curve.evaluate(k, t1, 1, false, false).unwrap();
    let h = [start[0], start[1] * dt, end[0], end[1] * dt];
    let mid = curve.evaluate(k, 0.5 * (t0 + t1), 0, true, false).unwrap()[0];
    let hermite_mid = (h[0] + h[2]) * 0.5 + (h[1] - h[3]) * 0.125;
    assert!(
        (mid - hermite_mid).length() <= 1e-12 * (1.0 + mid.length()),
        "[{t0}, {t1}] is not one cubic piece"
    );
    h
}

/// The largest certified bound, over every piece of the UV trim `trim` on `sphere`, of the
/// distance of its image from `reference`, with the number of pieces; panics where a
/// piece is not certified within `tol`. The pieces are the trim's spans, split at the
/// reference curve's knots. Against a cubic reference the trim runs on the edge's
/// parameter (`create_uv_trim_curve` keeps the two ranges one, checked), and the
/// certificate compares the image with the edge's own cubic piece at each parameter: its
/// bound is a bound of the distance from the edge. Against a circle the image is compared
/// with its own Hermite, and that cubic's distance from the circle is added
/// ([`bezier_off_circle`]).
fn certified_trim(
    k: &Kernel,
    sphere: &Surface,
    trim: &Curve,
    reference: &Reference<'_>,
    tol: f64,
) -> (f64, usize) {
    let CurveKind::BSpline(spline) = trim.kind() else {
        panic!("a sphere trim that is no B-spline");
    };
    assert!(!spline.is_rational().unwrap(), "a rational trim");
    assert!(spline.degree().unwrap() <= 3, "a trim of degree over 3");
    let mut ts = breaks(trim);
    let (lo, hi) = (ts[0], ts[ts.len() - 1]);
    if let Reference::Cubic(curve) = reference {
        let edge_breaks = breaks(curve);
        let (elo, ehi) = (edge_breaks[0], edge_breaks[edge_breaks.len() - 1]);
        assert!(
            (elo - lo).abs() <= 1e-12 && (ehi - hi).abs() <= 1e-12,
            "the trim [{lo}, {hi}] is not on the edge's parameter [{elo}, {ehi}]"
        );
        ts.extend(edge_breaks.into_iter().filter(|&t| t > lo && t < hi));
        ts.sort_by(f64::total_cmp);
        ts.dedup();
    }
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: false,
        non_zero_tangents: false,
    };
    // the image's point and derivative at `uv` with UV derivative `d`
    let image = |uv: Vec3, d: Vec3| {
        let m = sphere
            .evaluate(k, Point2::new(uv.x, uv.y), 1, 1, flags)
            .unwrap();
        (m[0][0], m[1][0] * d.x + m[0][1] * d.y)
    };
    let lift = |v: Vec3| Vec3::new(v.x, v.y, 0.0);
    let mut worst: f64 = 0.0;
    for pair in ts.windows(2) {
        let (t0, t1) = (pair[0], pair[1]);
        let uv = hermite(k, trim, t0, t1).map(lift);
        let bound = match reference {
            Reference::Cubic(curve) => {
                let xyz = hermite(k, curve, t0, t1);
                tightest(sphere, uv, xyz, tol).unwrap_or_else(|| {
                    panic!("[{t0}, {t1}]: the image is not certified within {tol:e} of the edge")
                })
            }
            Reference::Circle(circle, off) => {
                assert!(
                    *off <= 1e-9 * circle.radius,
                    "the edge is {off:e} off its circle"
                );
                // the piece cut in halves until each part's Hermite (of its image) is within
                // a quarter of the tolerance of the circle (its error from the image falls
                // as the fourth power of the part), each part certified on its Hermite
                let mut piece: f64 = 0.0;
                let mut parts = vec![(t0, t1, 0_u32)];
                while let Some((s0, s1, depth)) = parts.pop() {
                    let uv = hermite(k, trim, s0, s1).map(lift);
                    let (p0, d0) = image(uv[0], uv[1]);
                    let (p1, d1) = image(uv[2], uv[3]);
                    let bezier = [p0, p0 + d0 * (1.0 / 3.0), p1 - d1 * (1.0 / 3.0), p1];
                    let rest = bezier_off_circle(bezier, circle) + off;
                    if rest > 0.25 * tol && depth < MAX_HALVINGS {
                        let mid = 0.5 * (s0 + s1);
                        parts.push((s0, mid, depth + 1));
                        parts.push((mid, s1, depth + 1));
                        continue;
                    }
                    let b =
                        tightest(sphere, uv, [p0, d0, p1, d1], tol - rest).unwrap_or_else(|| {
                            panic!(
                            "[{s0}, {s1}]: the image is not certified within {:e} of its Hermite",
                            tol - rest
                        )
                        });
                    piece = piece.max(b + rest);
                }
                piece
            }
        };
        assert!(
            bound <= tol,
            "[{t0}, {t1}]: certified only {bound:e} from the edge (tolerance {tol:e})"
        );
        worst = worst.max(bound);
    }
    (worst, ts.len() - 1)
}

/// The most halvings of a trim piece against a circle reference.
const MAX_HALVINGS: u32 = 10;

/// The least bound [`sphere_span_image`] certifies for the span: asked within `budget`,
/// then again within half of each bound it gives (the bisection stops once a bound fits,
/// so a bound is near its budget), at most twice; `None` when not certified within
/// `budget`.
fn tightest(sphere: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], budget: f64) -> Option<f64> {
    let mut best = None;
    let mut ask = budget;
    for _ in 0..2 {
        match sphere_span_image(sphere, uv, xyz, ask) {
            SpanImage::Certified(b) => {
                best = Some(b);
                ask = 0.5 * b;
            }
            _ => break,
        }
    }
    best
}

/// One edge use on a sphere face of each edge whose curve is a circle or a non-rational
/// cubic B-spline (an edge's two uses on one sphere carry trims of one curve), with the
/// sphere, the edge's curve and its tolerance.
fn sphere_uses(brep: &Brep) -> Vec<(EdgeuseId, Surface, Curve, f64)> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for eu in solidcore::topology::brep::p2::edgeuses(brep, None).unwrap() {
        let Some(f) = edgeuse::p1::face(brep, eu).unwrap() else {
            continue;
        };
        let Some(sphere) = face::p2::surface(brep, f).unwrap().cloned() else {
            continue;
        };
        if !matches!(sphere.kind(), SurfaceKind::Sphere(_)) {
            continue;
        }
        let e = edgeuse::p1::edge(brep, eu).unwrap().unwrap();
        if !seen.insert(e) {
            continue;
        }
        let Some(curve) = edge::curve(brep, e).unwrap().cloned() else {
            continue;
        };
        if reference_of(&curve).is_none() {
            continue;
        }
        out.push((eu, sphere, curve, brep.edge(e).unwrap().tolerance));
    }
    out
}

/// The regression circle: the intersection of the unit sphere and the box `y <= -3e-5`
/// cuts the sphere in one circle 3e-5 from both poles. Each sphere trim of the circle has
/// every span certified within the tolerance 1e-5 of the circle (the first span was 5e-4
/// off before #18490).
#[test]
fn the_trims_of_a_circle_near_a_spheres_poles_are_certified_on_the_circle() {
    let delta = 3e-5;
    let tol = 1e-5;
    let mut k = Kernel::default();
    let a = api::create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0).unwrap();
    let b = api::create_box(&mut k, Point3::new(-2.0, -2.0, -2.0), 4.0, 2.0 - delta, 4.0).unwrap();
    let brep = boolean_intersection(&mut k, a, b).unwrap();
    let uses = sphere_uses(&brep);
    assert!(!uses.is_empty(), "no sphere trim of the circle");
    for (eu, sphere, curve, _) in &uses {
        let trim = edgeuse::p1::uv_trim_curve(&brep, *eu).unwrap().0.unwrap();
        let reference = reference_of(curve).unwrap();
        assert!(
            matches!(reference, Reference::Circle(..)),
            "the circle is no circle"
        );
        let (worst, pieces) = certified_trim(&k, sphere, trim, &reference, tol);
        eprintln!("regression circle: {pieces} pieces, certified within {worst:e}");
    }
}

/// An upstream test file read by the Rust reader, arena-named.
fn read(k: &mut Kernel, rel: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// Every sphere edge use of `file` whose edge is a circle or a non-rational cubic
/// B-spline and whose trim, made again by `create_uv_trim_curve`, is the drop's (cubic
/// spans): every piece certified within the edge's tolerance of the edge. The number of
/// edge uses checked.
fn certify_file(file: &str) -> Option<usize> {
    let mut k = Kernel::default();
    let mut brep = read(&mut k, file)?;
    let mut checked = 0;
    for (eu, sphere, curve, tol) in &sphere_uses(&brep) {
        let made = create_uv_trim_curve(&k, &mut brep, *eu, false, None).unwrap();
        // the drop's trims (cubic spans); a trim along an iso line is a line, no drop span
        let CurveKind::BSpline(trim) = made.curve.kind() else {
            continue;
        };
        if trim.degree().unwrap() < 3 {
            continue;
        }
        let reference = reference_of(curve).unwrap();
        let (worst, pieces) = certified_trim(&k, sphere, &made.curve, &reference, *tol);
        eprintln!("{file} {eu:?}: {pieces} pieces, certified within {worst:e} (tolerance {tol:e})");
        checked += 1;
    }
    Some(checked)
}

/// `Fillet/sphere_plane.smb` (the `own_reader_sphere_plane_*` goldens) and
/// `Fillet/sphere_diff_3cylinders.smb` (the `own_reader_sphere_diff_3cylinders_*` goldens
/// and the `stubs4` rounds on it), and `Solids/revolution_mold.smb` (whose four rebuilt sphere
/// trims the change refines): every sphere trim the change made finer is certified
/// within its edge's tolerance of the edge, piece by piece.
#[test]
fn the_moved_golden_families_sphere_trims_are_certified_on_their_edges() {
    for file in [
        "Fillet/sphere_plane.smb",
        "Fillet/sphere_diff_3cylinders.smb",
        "Solids/revolution_mold.smb",
    ] {
        if let Some(n) = certify_file(file) {
            assert!(
                n > 0,
                "{file}: no sphere edge use with a cubic or circular edge"
            );
        }
    }
}
