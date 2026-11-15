//! Not upstream (#16669): a certificate that a traced span's UV image on an analytic
//! sphere stays within a tolerance of the span's 3D curve.
//!
//! Upstream: none. `TestSpanAccuracy` (`source/SMLib/src/SmSurfaceIntersector.cpp`) samples
//! five interior parameters, and evaluates each UV point clamped to the domain. Near a
//! sphere's pole a span's UV Hermite cubic can leave the domain far behind (a span 0.37
//! long began at the seam crossing 3.3e-5 from the pole, with the exact chain-rule tangent
//! `u' ≈ δ/ρ²`, and its second control point was at u = −18.5). The clamped samples landed
//! near the pole and passed, but the trim made from the span missed its 3D curve by up to
//! 0.66.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`sphere_span_image`]      | none     |
//! | [`sphere_bezier_image`]    | none     |
//! | `net_box` (private)        | none     |
//!
//! Deviations: the bisection of the span's parameter is bounded at [`MAX_DEPTH`] levels (at
//! most `2^MAX_DEPTH − 1` bisections, so at most 511 piece bounds and 257 point values per
//! span, an explicit queue, no recursion). A span the bisection does not certify by then is
//! not certified, and the trace shortens its step. That fixed budget (at most 511 piece
//! bounds, 257 point values and one pass over the net's double knots per span) is why the
//! certificate does not poll for cancellation (`Kernel::check_cancelled`); its callers do,
//! once per trace step or try.

use std::collections::VecDeque;

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{ders_basis, Nurbs};
use crate::geom::vector3d::Vec3;
use crate::surface::surface::{Surface, SurfaceKind};

/// The deepest bisection of a span's parameter [`sphere_span_image`] makes: at most
/// `2^MAX_DEPTH` pieces.
pub const MAX_DEPTH: u32 = 8;

/// An enclosure of a point or vector of 3-space.
type Iv3 = [Ivl; 3];

/// What [`sphere_span_image`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpanImage {
    /// The surface is not an analytic sphere: no certificate is made.
    NotASphere,
    /// The image is within the returned bound of the 3D span, which is at most the tolerance.
    Certified(f64),
    /// No certificate within the tolerance, for the reason given.
    NotCertified(Refusal),
}

/// Why [`sphere_span_image`] gave no certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// At some parameter the image is farther from the 3D span than the tolerance: the
    /// enclosure of the distance there lies wholly over it.
    Off,
    /// A piece at the deepest bisection ([`MAX_DEPTH`]) did not certify.
    Depth,
    /// An enclosure failed: the net has no NURBS data, a weight's enclosure was not
    /// positive, or the net is not C¹ across a knot of full multiplicity (or is
    /// discontinuous across one of higher multiplicity).
    Enclosure,
}

/// Not upstream (#16669): whether the image on `surface` of the UV Hermite span
/// `uv = [uv0, duv0, uv1, duv1]` (from `uv0` with derivative `duv0` to `uv1` with `duv1`,
/// its parameter in `[0, 1]`) lies within `tol` of the 3D Hermite span `xyz` (the same
/// layout) at every parameter, on an analytic sphere. The UV points are in `x`, `y` (`z`
/// zero); the derivatives are those the trace scales by its step, as its span test does.
///
/// Both spans are cubic Bezier curves (their control points enclosed in rounded intervals
/// from the Hermite data). The image is the one `Surface::evaluate` gives: it evaluates a
/// sphere through its rational net (its `SmBSplineSurface` base) at the UV point clamped to
/// the net's knot range, so the image is `S(c(N(t)))`, `c` the clamp. Let
/// `D(t) = S(N(t)) − H(t)` with `S` the net's patches, the end patches continued past the
/// knots' range. On each piece `[a, b]` of the parameter, of length `w`, each component of
/// `D` lies in `hull(D(a), D(b)) + D''([a, b]) [−w²/8, 0]` (`piece_bound`), with
/// `D'' = S_uu N_u'² + 2 S_uv N_u' N_v' + S_vv N_v'² + S_u N_u'' + S_v N_v'' − H''`, the net's
/// derivatives enclosed over a UV box holding `N([a, b])` and its clamp (the hull of the
/// piece's control points) and the spans' derivatives over the piece. Where that box leaves
/// the knots' range by `o_u` and `o_v`, `S(c(N)) − S(N)` adds `S_u [−o_u, o_u] +
/// S_v [−o_v, o_v]` per component (the mean value theorem along the segment to the clamped
/// point, which stays in the box). The bound is the length of the enclosure's farthest
/// corner, rounded up. Pieces that do not certify are bisected, at most [`MAX_DEPTH`] times;
/// a parameter where the clamped image's distance lies wholly over `tol` ends the search
/// ([`Refusal::Off`]).
///
/// At a knot of full multiplicity (a double knot of the sphere's quadratic net) the
/// patches meet only C⁰ in the homogeneous net. `joints` bounds, once per span, the jump
/// `J` of the rational first derivative across each such knot: the one-sided homogeneous
/// derivatives' difference on each row, less the multiple `λ P_i` of the row's control
/// point on the knot, bounds it by `max (|r_xyz| + |r_w| M) / w_min` (`M` bounding the net's
/// points, `w_min` its least weight), and a net with `J` over `1e-9 (1 + M)` is refused
/// ([`Refusal::Enclosure`]). Where the piece's UV box holds such a knot, `D'` may jump by
/// `J |N'|` at each of at most three crossings, which adds `3 J |N'| w / 4` per component.
///
/// PROVEN: the piece bound is the Peano kernel form of linear interpolation's remainder,
/// `D(t) − L(t) = −∫ G(t, s) D''(s) ds` with `0 ≤ G` and `∫ G(t, s) ds = (t − a)(b − t) / 2
/// ≤ w² / 8`, for the part of `D` that is C¹ and piecewise C² (the pieces of `D''` hulled
/// over every patch the box meets), plus the interpolation error of each kink
/// `J (t − s)₊`, at most `J w / 4`; the convex hull property; outward rounding throughout.
/// The canonical sphere's `J` is of rounding size (the unit test
/// `the_canonical_net_is_c1_at_its_double_knots`).
///
/// [`SpanImage::NotASphere`] for any other surface kind; [`SpanImage::NotCertified`] with
/// the [`Refusal`].
#[must_use]
pub fn sphere_span_image(surface: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], tol: f64) -> SpanImage {
    certify(surface, bezier(uv), bezier(xyz), tol)
}

/// Not upstream (#16669): [`sphere_span_image`] for spans given as cubic Bezier control
/// points (`uv` and `xyz`, the parameter in `[0, 1]`), as a curve's control polygon holds
/// them after knot insertion.
#[must_use]
pub fn sphere_bezier_image(
    surface: &Surface,
    uv: [Vec3; 4],
    xyz: [Vec3; 4],
    tol: f64,
) -> SpanImage {
    let iv = |v: Vec3| [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)];
    certify(surface, uv.map(iv), xyz.map(iv), tol)
}

/// The certificate of [`sphere_span_image`] for enclosed Bezier control points.
fn certify(surface: &Surface, n: [Iv3; 4], h: [Iv3; 4], tol: f64) -> SpanImage {
    let SurfaceKind::Sphere(sphere) = surface.kind() else {
        return SpanImage::NotASphere;
    };
    let Ok(nurbs) = Nurbs::of(sphere.base().base()) else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    let domain = (
        knot_range(&nurbs.u, nurbs.p, nurbs.nu),
        knot_range(&nurbs.v, nurbs.q, nurbs.nv),
    );
    let (Some(du), Some(dv)) = domain else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    // both sides moved by the 3D span's start, so the enclosures of the net's quotients
    // are not widened by the size of the coordinates
    let [cx, cy, cz] = h[0].map(Ivl::mid);
    let origin = [Ivl::from(cx), Ivl::from(cy), Ivl::from(cz)];
    let mut pw = Vec::new();
    for c in nurbs.pw.chunks_exact(4) {
        if let [x, y, z, w] = *c {
            let w = Ivl::from(w);
            pw.extend([
                Ivl::from(x) - origin[0] * w,
                Ivl::from(y) - origin[1] * w,
                Ivl::from(z) - origin[2] * w,
                w,
            ]);
        }
    }
    let (Some(ju), Some(jv)) = (joints(&nurbs, &pw, true), joints(&nurbs, &pw, false)) else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    let net = Shifted {
        nurbs,
        pw,
        du,
        dv,
        ju,
        jv,
    };
    let h = h.map(|q| [q[0] - origin[0], q[1] - origin[1], q[2] - origin[2]]);
    let curves = Curves {
        dn: derivative(&n),
        ddn: derivative(&derivative(&n)),
        ddh: derivative(&derivative(&h)),
        n: n.to_vec(),
        h: h.to_vec(),
    };
    let ends = (
        point_value(&net, &curves, 0.0),
        point_value(&net, &curves, 1.0),
    );
    let (Some(d0), Some(d1)) = ends else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    if off(d0.clamped, tol) || off(d1.clamped, tol) {
        return SpanImage::NotCertified(Refusal::Off);
    }
    let mut worst = 0.0_f64;
    // breadth first: a span off the curve is found at a coarse piece's middle before the
    // pieces of a level are refined
    let mut queue = VecDeque::from([(0.0_f64, d0.continued, 1.0_f64, d1.continued, 0_u32)]);
    while let Some((a, da, b, db, depth)) = queue.pop_front() {
        // a missing bound (an enclosure that failed) does not certify
        if let Some(bound) = piece_bound(&net, &curves, a, da, b, db) {
            if bound <= tol {
                worst = worst.max(bound);
                continue;
            }
        }
        if depth >= MAX_DEPTH {
            return SpanImage::NotCertified(Refusal::Depth);
        }
        let m = 0.5 * (a + b);
        let Some(dm) = point_value(&net, &curves, m) else {
            return SpanImage::NotCertified(Refusal::Enclosure);
        };
        if off(dm.clamped, tol) {
            return SpanImage::NotCertified(Refusal::Off);
        }
        queue.push_back((a, da, m, dm.continued, depth + 1));
        queue.push_back((m, dm.continued, b, db, depth + 1));
    }
    SpanImage::Certified(worst)
}

/// The range `[knots[deg], knots[count]]` of the knot vector `knots` (degree `deg`, `count`
/// control points): the net's natural domain in that direction, which the evaluator clamps
/// to.
fn knot_range(knots: &[f64], deg: usize, count: usize) -> Option<(f64, f64)> {
    Some((*knots.get(deg)?, *knots.get(count)?))
}

/// The enclosure `x` clamped to `[lo, hi]` (the clamp is monotone, so its ends are clamped).
fn clamp_ivl(x: Ivl, (lo, hi): (f64, f64)) -> Ivl {
    Ivl::new(x.lo.max(lo).min(hi), x.hi.max(lo).min(hi))
}

/// How far the enclosure `x` reaches past `[lo, hi]`, rounded up (0 within it).
fn overshoot(x: Ivl, (lo, hi): (f64, f64)) -> f64 {
    let below = (Ivl::from(lo) - Ivl::from(x.lo)).hi;
    let above = (Ivl::from(x.hi) - Ivl::from(hi)).hi;
    below.max(above).max(0.0)
}

/// The sphere's net moved by `−c`: its homogeneous control points `(x w − c_x w, …, w)`,
/// enclosed, its knot ranges `du`, `dv` (the natural domain), and its C⁰ joints in u and v.
struct Shifted {
    nurbs: Nurbs,
    pw: Vec<Ivl>,
    du: (f64, f64),
    dv: (f64, f64),
    ju: Vec<Joint>,
    jv: Vec<Joint>,
}

/// A knot `at` of full multiplicity, where the net's patches meet C⁰ in the homogeneous
/// net, and a bound `jump` on the jump of the rational first derivative across it.
#[derive(Clone, Copy, Debug)]
struct Joint {
    at: f64,
    jump: f64,
}

/// The joints of the net `nurbs` (homogeneous control points `pw`, enclosed) across u
/// (`across_u`) or v: for each interior knot of multiplicity equal to the degree, with the
/// control point `P_i` on it, the one-sided homogeneous derivatives `p (P_i − P_{i−1}) / h_l`
/// and `p (P_{i+1} − P_i) / h_r` on every row; their difference less `λ P_i` (`λ` from the
/// first row) leaves `r`, and the rational derivative's jump is at most
/// `max_rows (|r_xyz| + |r_w| M) / w_min`. `None` for a knot of higher multiplicity (the net
/// is discontinuous there), a weight's enclosure that is not positive, or a jump over
/// `1e-9 (1 + M)` (the net is not C¹ there).
fn joints(nurbs: &Nurbs, pw: &[Ivl], across_u: bool) -> Option<Vec<Joint>> {
    let (knots, deg, count, rows) = if across_u {
        (&nurbs.u, nurbs.p, nurbs.nu, nurbs.nv)
    } else {
        (&nurbs.v, nurbs.q, nurbs.nv, nurbs.nu)
    };
    let point = |along: usize, row: usize| -> Option<[Ivl; 4]> {
        let (i, j) = if across_u { (along, row) } else { (row, along) };
        let l = 4 * (i * nurbs.nv + j);
        Some([
            *pw.get(l)?,
            *pw.get(l + 1)?,
            *pw.get(l + 2)?,
            *pw.get(l + 3)?,
        ])
    };
    // M bounds |S| (the convex hull property, weights positive); w_min bounds the weight
    let mut big = Ivl::from(0.0);
    let mut w_min = f64::INFINITY;
    for c in pw.chunks_exact(4) {
        if let [x, y, z, w] = *c {
            if !w.is_positive() {
                return None;
            }
            w_min = w_min.min(w.lo);
            let r = ((x / w) * (x / w) + (y / w) * (y / w) + (z / w) * (z / w)).sqrt();
            big = big.hull(r);
        }
    }
    let m = big.hi;
    let p = Ivl::from(f64::from(u32::try_from(deg).ok()?));
    let (lo, hi) = (*knots.get(deg)?, *knots.get(count)?);
    let mut out = Vec::new();
    let mut f = deg + 1;
    while f < count {
        let k = *knots.get(f)?;
        let mut mult = 1;
        while knots.get(f + mult) == Some(&k) {
            mult += 1;
        }
        let interior = lo < k && k < hi;
        if interior && mult > deg {
            return None;
        }
        if interior && mult == deg {
            let i = f.checked_sub(1)?;
            let hl = Ivl::from(k) - Ivl::from(*knots.get(i)?);
            let hr = Ivl::from(*knots.get(f + deg)?) - Ivl::from(k);
            let mut lambda = None;
            let mut jump = 0.0_f64;
            for row in 0..rows {
                let (pm, pi, pp) = (
                    point(i.checked_sub(1)?, row)?,
                    point(i, row)?,
                    point(i + 1, row)?,
                );
                let d: Vec<Ivl> = pm
                    .iter()
                    .zip(&pi)
                    .zip(&pp)
                    .map(|((&a, &b), &c)| p * (c - b) / hr - p * (b - a) / hl)
                    .collect();
                let [_, _, _, piw] = pi;
                let l = *lambda.get_or_insert(d.get(3)?.mid() / piw.mid());
                let l = Ivl::from(l);
                let r: Vec<Ivl> = d.iter().zip(&pi).map(|(&a, &b)| a - l * b).collect();
                let (rx, ry, rz, rw) = (*r.first()?, *r.get(1)?, *r.get(2)?, *r.get(3)?);
                let rxyz = (rx * rx + ry * ry + rz * rz).sqrt();
                let bound = (rxyz + Ivl::from(rw.mag()) * Ivl::from(m)) / Ivl::from(w_min);
                jump = jump.max(bound.hi);
            }
            let c1 = jump <= 1e-9 * (1.0 + m);
            if !c1 {
                return None;
            }
            out.push(Joint { at: k, jump });
        }
        f += mult;
    }
    Some(out)
}

/// The kink term of a piece of length `w` whose UV box spans `bu` × `bv`, the UV span's
/// first derivative within `nu`, `nv` over it: `3 J |N'| w / 4` for each joint in the box.
fn kinks(net: &Shifted, bu: Ivl, bv: Ivl, nu: Ivl, nv: Ivl, w: Ivl) -> f64 {
    let three_quarters = Ivl::from(0.75);
    let mut sum = Ivl::from(0.0);
    for (joints, b, d) in [(&net.ju, bu, nu), (&net.jv, bv, nv)] {
        for j in joints {
            if b.lo <= j.at && j.at <= b.hi {
                sum = sum + three_quarters * Ivl::from(j.jump) * Ivl::from(d.mag()) * w;
            }
        }
    }
    sum.hi
}

/// The two spans' Bezier control points and those of the UV span's first and second
/// derivatives and the 3D span's second.
struct Curves {
    n: Vec<Iv3>,
    h: Vec<Iv3>,
    dn: Vec<Iv3>,
    ddn: Vec<Iv3>,
    ddh: Vec<Iv3>,
}

/// Whether the enclosure `d` of a difference lies wholly over `tol` in length.
fn off(d: Iv3, tol: f64) -> bool {
    let mut least = Ivl::from(0.0);
    for c in d {
        let low = Ivl::from(c.mig());
        least = least + low * low;
    }
    least.sqrt().lo > tol
}

/// Enclosures of the difference at a parameter: of the clamped image's,
/// `S(c(N(t))) − H(t)`, and of the continued net's, `D(t) = S(N(t)) − H(t)`.
#[derive(Clone, Copy)]
struct PointValue {
    clamped: Iv3,
    continued: Iv3,
}

/// The [`PointValue`] at `t`; the continued value is evaluated apart only where `N(t)`'s
/// enclosure leaves the knots' range.
fn point_value(net: &Shifted, c: &Curves, t: f64) -> Option<PointValue> {
    let nt = blossom(&c.n, &[t; 3])?;
    let ht = blossom(&c.h, &[t; 3])?;
    let minus = |s: Iv3| [s[0] - ht[0], s[1] - ht[1], s[2] - ht[2]];
    let (cu, cv) = (clamp_ivl(nt[0], net.du), clamp_ivl(nt[1], net.dv));
    let clamped = minus(net_box(net, cu, cv, 0)?.s);
    let inside = cu == nt[0] && cv == nt[1];
    let continued = if inside {
        clamped
    } else {
        minus(net_box(net, nt[0], nt[1], 0)?.s)
    };
    Some(PointValue { clamped, continued })
}

/// The cubic Bezier control points of the Hermite span `[p0, d0, p1, d1]` (from `p0` with
/// derivative `d0` to `p1` with `d1`, on `[0, 1]`), enclosed.
fn bezier([p0, d0, p1, d1]: [Vec3; 4]) -> [Iv3; 4] {
    let iv = |v: Vec3| [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)];
    let third = |p: Iv3, d: Iv3, sign: f64| {
        let three = Ivl::from(3.0);
        let s = Ivl::from(sign);
        [
            p[0] + s * d[0] / three,
            p[1] + s * d[1] / three,
            p[2] + s * d[2] / three,
        ]
    };
    let (q0, q3) = (iv(p0), iv(p1));
    [q0, third(q0, iv(d0), 1.0), third(q3, iv(d1), -1.0), q3]
}

/// The control points of a Bezier curve's derivative (one degree lower).
fn derivative(q: &[Iv3]) -> Vec<Iv3> {
    // (a cubic at most: the degree is exact)
    let degree = Ivl::from(f64::from(
        u8::try_from(q.len().saturating_sub(1)).unwrap_or(0),
    ));
    q.windows(2)
        .filter_map(|w| match w {
            [a, b] => Some([
                degree * (b[0] - a[0]),
                degree * (b[1] - a[1]),
                degree * (b[2] - a[2]),
            ]),
            _ => None,
        })
        .collect()
}

/// The blossom (polar form) of the Bezier curve `q` at the arguments `ts` (as many as its
/// degree), enclosed: de Casteljau's algorithm with argument `ts[l]` at level `l`. With
/// every argument `t` it is the curve's point at `t`.
fn blossom(q: &[Iv3], ts: &[f64]) -> Option<Iv3> {
    let mut work = q.to_vec();
    for level in 1..work.len() {
        let t = Ivl::from(*ts.get(level - 1)?);
        let s = Ivl::from(1.0) - t;
        for i in 0..work.len() - level {
            let (a, b) = (*work.get(i)?, *work.get(i + 1)?);
            let slot = work.get_mut(i)?;
            *slot = [
                s * a[0] + t * b[0],
                s * a[1] + t * b[1],
                s * a[2] + t * b[2],
            ];
        }
    }
    work.first().copied()
}

/// An enclosure of the Bezier curve `q` over `[a, b]`: the hull of the control points of
/// its piece there (the blossoms at `a` and `b` repeated), which hold the piece (the convex
/// hull property). The arguments are exact, so only rounding widens the enclosure, not
/// the size of the coordinates (de Casteljau's algorithm with an interval argument widens
/// it by their size).
fn piece_hull(q: &[Iv3], a: f64, b: f64) -> Option<Iv3> {
    let degree = q.len().checked_sub(1)?;
    let mut out: Option<Iv3> = None;
    for j in 0..=degree {
        let ts: Vec<f64> = (0..degree).map(|i| if i < j { b } else { a }).collect();
        let c = blossom(q, &ts)?;
        out = Some(match out {
            None => c,
            Some(o) => hull3(o, c),
        });
    }
    out
}

/// The bound of `|S(c(N(t))) − H(t)|` over `[a, b]` ([`sphere_span_image`]) from the
/// continued differences `da` and `db` at the ends, rounded up: per component
/// `hull(D(a), D(b)) + D''(B) [−w²/8, 0] + S_u(B) [−o_u, o_u] + S_v(B) [−o_v, o_v]`,
/// `w = b − a`, `B` the hull of the piece's UV box and its clamp, `o_u` and `o_v` how far the
/// box leaves the knots' range, plus the kink term of the joints in the box (`kinks`).
/// `None` where an enclosure failed.
fn piece_bound(net: &Shifted, c: &Curves, a: f64, da: Iv3, b: f64, db: Iv3) -> Option<f64> {
    let w = Ivl::from(b) - Ivl::from(a);
    let sag = Ivl::new(-(w * w * Ivl::from(0.125)).hi, 0.0);
    let nbox = piece_hull(&c.n, a, b)?;
    let dnbox = piece_hull(&c.dn, a, b)?;
    let ddnbox = piece_hull(&c.ddn, a, b)?;
    let ddhbox = piece_hull(&c.ddh, a, b)?;
    let (ou, ov) = (overshoot(nbox[0], net.du), overshoot(nbox[1], net.dv));
    let bu = nbox[0].hull(clamp_ivl(nbox[0], net.du));
    let bv = nbox[1].hull(clamp_ivl(nbox[1], net.dv));
    let over = net_box(net, bu, bv, 2)?;
    let two = Ivl::from(2.0);
    let (nu, nv) = (dnbox[0], dnbox[1]);
    let (spread_u, spread_v) = (Ivl::new(-ou, ou), Ivl::new(-ov, ov));
    let kink = kinks(net, bu, bv, nu, nv, w);
    let kink = Ivl::new(-kink, kink);
    let mut sum = Ivl::from(0.0);
    for i in 0..3 {
        let su = *over.su.get(i)?;
        let sv = *over.sv.get(i)?;
        let d2 = *over.suu.get(i)? * nu * nu
            + two * *over.suv.get(i)? * nu * nv
            + *over.svv.get(i)? * nv * nv
            + su * ddnbox[0]
            + sv * ddnbox[1]
            - *ddhbox.get(i)?;
        let clamp = su * spread_u + sv * spread_v;
        let e = Ivl::from((da.get(i)?.hull(*db.get(i)?) + d2 * sag + clamp + kink).mag());
        sum = sum + e * e;
    }
    let bound = sum.sqrt().hi;
    bound.is_finite().then_some(bound)
}

/// The spans `k` of the knot vector `knots` (degree `deg`, `count` control points) whose
/// pieces meet `x`, the end spans continued past the knots' range.
fn spans_meeting(knots: &[f64], deg: usize, count: usize, x: Ivl) -> Vec<usize> {
    let mut out = Vec::new();
    for k in deg..count {
        let (Some(&lo), Some(&hi)) = (knots.get(k), knots.get(k + 1)) else {
            continue;
        };
        let first = k == deg;
        let last = k + 1 == count;
        let meets = hi > lo && (first || x.hi >= lo) && (last || x.lo <= hi);
        if meets {
            out.push(k);
        }
    }
    out
}

/// Enclosures of the sphere net's point and its first and second derivatives.
#[derive(Clone, Copy)]
struct Ders {
    s: Iv3,
    su: Iv3,
    sv: Iv3,
    suu: Iv3,
    suv: Iv3,
    svv: Iv3,
}

impl Ders {
    /// The componentwise hull of two enclosures.
    fn hull(self, o: Self) -> Self {
        Self {
            s: hull3(self.s, o.s),
            su: hull3(self.su, o.su),
            sv: hull3(self.sv, o.sv),
            suu: hull3(self.suu, o.suu),
            suv: hull3(self.suv, o.suv),
            svv: hull3(self.svv, o.svv),
        }
    }
}

/// Enclosures of the sphere net's point and derivatives over the UV box `u` × `v`: the hull
/// over every polynomial patch the box meets, each evaluated over the box's part in it (the
/// end patches continued past the knots' range). `None` where a weight's enclosure is not
/// positive or no patch meets the box. Derivatives past `order` are not computed and must
/// not be read.
fn net_box(net: &Shifted, u: Ivl, v: Ivl, order: usize) -> Option<Ders> {
    let mut out: Option<Ders> = None;
    for ku in spans_meeting(&net.nurbs.u, net.nurbs.p, net.nurbs.nu, u) {
        let uu = clip(&net.nurbs.u, net.nurbs.p, net.nurbs.nu, ku, u)?;
        for kv in spans_meeting(&net.nurbs.v, net.nurbs.q, net.nurbs.nv, v) {
            let vv = clip(&net.nurbs.v, net.nurbs.q, net.nurbs.nv, kv, v)?;
            let patch = patch_box(net, ku, kv, uu, vv, order)?;
            out = Some(match out {
                None => patch,
                Some(o) => o.hull(patch),
            });
        }
    }
    out
}

/// The part of `x` in span `k` (all of it past the knots' range at an end span).
fn clip(knots: &[f64], deg: usize, count: usize, k: usize, x: Ivl) -> Option<Ivl> {
    let lo = if k == deg {
        x.lo
    } else {
        x.lo.max(*knots.get(k)?)
    };
    let hi = if k + 1 == count {
        x.hi
    } else {
        x.hi.min(*knots.get(k + 1)?)
    };
    let ordered = lo <= hi;
    ordered.then(|| Ivl::new(lo, hi))
}

/// The componentwise hull of two enclosures.
fn hull3(a: Iv3, b: Iv3) -> Iv3 {
    [a[0].hull(b[0]), a[1].hull(b[1]), a[2].hull(b[2])]
}

/// The point and derivatives of the net's patch `(ku, kv)` over `u` × `v`, enclosed: the
/// homogeneous sums `A` (weight `w`) and the quotient rule, `S = A / w`,
/// `S_u = (A_u − w_u S) / w`, `S_uu = (A_uu − 2 w_u S_u − w_uu S) / w` and
/// `S_uv = (A_uv − w_u S_v − w_v S_u − w_uv S) / w`; the
/// derivatives past `order` are not computed and must not be read.
fn patch_box(net: &Shifted, ku: usize, kv: usize, u: Ivl, v: Ivl, order: usize) -> Option<Ders> {
    let order = order.min(2);
    let nu = ders_basis(&net.nurbs.u, ku, net.nurbs.p, u, order);
    let nv = ders_basis(&net.nurbs.v, kv, net.nurbs.q, v, order);
    let basis = |m: &Vec<Vec<Ivl>>, a: usize, i: usize| m.get(a).and_then(|r| r.get(i)).copied();
    let mut h = [[[Ivl::from(0.0); 4]; 3]; 3];
    for (a, row) in h.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            if a + b > order {
                continue;
            }
            for i in 0..=net.nurbs.p {
                for j in 0..=net.nurbs.q {
                    let li = 4
                        * ((ku + i).checked_sub(net.nurbs.p)? * net.nurbs.nv
                            + (kv + j).checked_sub(net.nurbs.q)?);
                    let c = basis(&nu, a, i)? * basis(&nv, b, j)?;
                    for (e, he) in hab.iter_mut().enumerate() {
                        *he = *he + c * *net.pw.get(li + e)?;
                    }
                }
            }
        }
    }
    let [[h00, h01, h02], [h10, h11, _], [h20, _, _]] = h;
    let w = h00[3];
    let positive = w.lo > 0.0;
    if !positive {
        return None;
    }
    let two = Ivl::from(2.0);
    let s = [h00[0] / w, h00[1] / w, h00[2] / w];
    let first = |d: [Ivl; 4]| {
        [
            (d[0] - d[3] * s[0]) / w,
            (d[1] - d[3] * s[1]) / w,
            (d[2] - d[3] * s[2]) / w,
        ]
    };
    let (su, sv) = (first(h10), first(h01));
    // (A_ab − w_a S_b − w_b S_a − w_ab S) / w, componentwise
    let second = |d: [Ivl; 4], wa: Ivl, sa: Iv3, wb: Ivl, sb: Iv3| {
        let [d0, d1, d2, dw] = d;
        [
            (d0 - wa * sb[0] - wb * sa[0] - dw * s[0]) / w,
            (d1 - wa * sb[1] - wb * sa[1] - dw * s[1]) / w,
            (d2 - wa * sb[2] - wb * sa[2] - dw * s[2]) / w,
        ]
    };
    // S_uu: w_a = w_b = w_u and S_a = S_b = S_u, the middle terms 2 w_u S_u
    let pure = |d: [Ivl; 4], wa: Ivl, sa: Iv3| {
        let [d0, d1, d2, dw] = d;
        [
            (d0 - two * wa * sa[0] - dw * s[0]) / w,
            (d1 - two * wa * sa[1] - dw * s[1]) / w,
            (d2 - two * wa * sa[2] - dw * s[2]) / w,
        ]
    };
    let suu = pure(h20, h10[3], su);
    let svv = pure(h02, h01[3], sv);
    let suv = second(h11, h10[3], su, h01[3], sv);
    Some(Ders {
        s,
        su,
        sv,
        suu,
        suv,
        svv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Point2;
    use crate::kernel::Kernel;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use crate::surface::sphere::Sphere;

    fn ball(k: &Kernel) -> Surface {
        Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(1.0, -2.0, 3.0),
                    Vec3::new(0.0, 0.6, 0.8),
                    Vec3::new(1.0, 0.0, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        )
    }

    /// The point at `t` of the Hermite span `[p0, d0, p1, d1]` on `[0, 1]`.
    fn hermite([p0, d0, p1, d1]: [Vec3; 4], t: f64) -> Vec3 {
        let (t2, t3) = (t * t, t * t * t);
        p0 * (2.0 * t3 - 3.0 * t2 + 1.0)
            + d0 * (t3 - 2.0 * t2 + t)
            + p1 * (-2.0 * t3 + 3.0 * t2)
            + d1 * (t3 - t2)
    }

    /// A UV span on the ball from `a` to `b` (its chord as both derivatives) and the 3D
    /// Hermite span through the image's end points with the chain-rule derivatives.
    fn spans(k: &Kernel, s: &Surface, a: Point2, b: Point2) -> ([Vec3; 4], [Vec3; 4]) {
        let d = b - a;
        let image = |p: Point2| {
            let m = s.evaluate(k, p, 1, 1, EvaluateFlags::default()).unwrap();
            (m[0][0], m[1][0] * d.x + m[0][1] * d.y)
        };
        let ((p0, d0), (p1, d1)) = (image(a), image(b));
        let lift = |p: Point2| Vec3::new(p.x, p.y, 0.0);
        let dv = Vec3::new(d.x, d.y, 0.0);
        ([lift(a), dv, lift(b), dv], [p0, d0, p1, d1])
    }

    /// The largest distance between the image and the 3D span at `n + 1` parameters.
    fn sampled(k: &Kernel, s: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], n: u32) -> f64 {
        (0..=n)
            .map(|i| {
                let t = f64::from(i) / f64::from(n);
                let q = hermite(uv, t);
                let p = s.evaluate_point(k, Point2::new(q.x, q.y)).unwrap();
                p.distance(hermite(xyz, t))
            })
            .fold(0.0, f64::max)
    }

    /// Whether the certificate is sound on the span: at the tolerance `tol` it certifies
    /// with a bound holding every sampled distance, and at 0.99 of the largest sampled
    /// distance (below the true largest) it does not.
    fn sound(k: &Kernel, s: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], tol: f64) -> f64 {
        let seen = sampled(k, s, uv, xyz, 4000);
        match sphere_span_image(s, uv, xyz, tol) {
            SpanImage::Certified(bound) => {
                assert!(seen <= bound && bound <= tol, "{seen} {bound} {tol}");
            }
            other => panic!("{other:?}, sampled {seen}, tolerance {tol}"),
        }
        assert!(
            matches!(
                sphere_span_image(s, uv, xyz, 0.99 * seen),
                SpanImage::NotCertified(_)
            ),
            "certified below the sampled {seen}"
        );
        seen
    }

    /// The bound holds the sampled distances and is at most the tolerance, on spans inside
    /// one patch, across a knot, and near a pole; below the largest sampled distance there
    /// is no certificate.
    #[test]
    fn the_bound_holds_every_sampled_distance() {
        let k = &Kernel::default();
        let s = ball(k);
        let cases = [
            (Point2::new(0.05, 0.3), Point2::new(0.1, 0.4)),
            (Point2::new(0.23, 0.48), Point2::new(0.27, 0.52)),
            (Point2::new(0.6, 0.97), Point2::new(0.62, 0.999)),
            (Point2::new(0.77, 0.15), Point2::new(0.73, 0.17)),
        ];
        for (a, b) in cases {
            let (uv, xyz) = spans(k, &s, a, b);
            let seen = sound(k, &s, uv, xyz, 1e-2);
            assert!(seen < 1e-2 / 4.0, "{a:?} {b:?}: sampled {seen}");
        }
    }

    /// A span whose UV Hermite crosses the seam u = 0, so the evaluator clamps part of it to
    /// u = 0: the bound holds the clamped image's distances, and below them there is no
    /// certificate.
    #[test]
    fn a_span_crossing_the_seam_is_bounded_with_its_clamp() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(k, &s, Point2::new(0.01, 0.4), Point2::new(-0.01, 0.42));
        let seen = sampled(k, &s, uv, xyz, 4000);
        assert!(seen > 1e-3, "the clamp moves the image: {seen}");
        sound(k, &s, uv, xyz, 1.0);
    }

    /// Spans reaching the pole v = 1 and running past it: the bound holds the clamped
    /// image's distances, and below them there is no certificate.
    #[test]
    fn a_span_reaching_the_pole_is_bounded_with_its_clamp() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(k, &s, Point2::new(0.3, 0.98), Point2::new(0.31, 1.0));
        sound(k, &s, uv, xyz, 1e-2);
        let (uv, xyz) = spans(k, &s, Point2::new(0.3, 0.98), Point2::new(0.31, 1.02));
        sound(k, &s, uv, xyz, 1.0);
    }

    /// A UV span whose second control point leaves the domain far behind (as the #16669
    /// span's did, u = −18.5) is not certified, though its ends are on the 3D span.
    #[test]
    fn a_span_whose_uv_image_leaves_the_curve_is_not_certified() {
        let k = &Kernel::default();
        let s = ball(k);
        let (mut uv, xyz) = spans(k, &s, Point2::new(0.97, 0.9), Point2::new(0.99, 0.99));
        uv[1] = Vec3::new(-30.0, uv[1].y, 0.0);
        assert!(sampled(k, &s, uv, xyz, 2000) > 0.1);
        assert!(matches!(
            sphere_span_image(&s, uv, xyz, 1e-2),
            SpanImage::NotCertified(_)
        ));
    }

    /// A tolerance of zero on a span so short that every point's distance encloses 0: no
    /// point is off, and the bisection ends at its depth bound.
    #[test]
    fn a_zero_tolerance_ends_at_the_depth_bound() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(
            k,
            &s,
            Point2::new(0.2, 0.3),
            Point2::new(0.2 + 1e-7, 0.3 + 1e-7),
        );
        assert_eq!(
            sphere_span_image(&s, uv, xyz, 0.0),
            SpanImage::NotCertified(Refusal::Depth)
        );
    }

    /// The net's homogeneous control points, enclosed.
    fn enclosed(net: &Nurbs) -> Vec<Ivl> {
        net.pw.iter().copied().map(Ivl::from).collect()
    }

    /// The canonical sphere's net is C¹ across its four double knots (three in u, one in
    /// v): `joints` finds each, with a derivative jump of rounding size.
    #[test]
    fn the_canonical_net_is_c1_at_its_double_knots() {
        let k = &Kernel::default();
        let s = ball(k);
        let SurfaceKind::Sphere(sphere) = s.kind() else {
            panic!("a sphere")
        };
        let net = Nurbs::of(sphere.base().base()).unwrap();
        let pw = enclosed(&net);
        let ju = joints(&net, &pw, true).unwrap();
        let jv = joints(&net, &pw, false).unwrap();
        assert_eq!((ju.len(), jv.len()), (3, 1), "{ju:?} {jv:?}");
        for j in ju.iter().chain(&jv) {
            assert!(j.jump < 1e-12, "{j:?}");
        }
    }

    /// A quadratic net (degree 1 across) whose double knot at u = 1/2 joins two straight
    /// spans: C¹ when the knots split the two spans' lengths alike, refused when they do
    /// not (unequal arcs: the speed jumps) or when the spans turn a corner.
    #[test]
    fn a_net_that_is_only_c0_at_a_double_knot_is_refused() {
        let net = |u: Vec<f64>, pts: [[f64; 3]; 5]| {
            let mut pw = Vec::new();
            for p in pts {
                for dz in [0.0, 1.0] {
                    pw.extend([p[0], p[1], p[2] + dz, 1.0]);
                }
            }
            Nurbs {
                p: 2,
                q: 1,
                nu: 5,
                nv: 2,
                u,
                v: vec![0.0, 0.0, 1.0, 1.0],
                pw,
            }
        };
        let straight = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
        ];
        let even = net(vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], straight);
        let j = joints(&even, &enclosed(&even), true).unwrap();
        assert!(j.len() == 1 && j[0].jump < 1e-12, "{j:?}");
        let uneven = net(vec![0.0, 0.0, 0.0, 0.25, 0.25, 1.0, 1.0, 1.0], straight);
        assert!(joints(&uneven, &enclosed(&uneven), true).is_none());
        let corner = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
        ];
        let turned = net(vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], corner);
        assert!(joints(&turned, &enclosed(&turned), true).is_none());
    }

    /// Other surface kinds get no certificate.
    #[test]
    fn a_plane_is_not_a_sphere() {
        let s = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let z = [Vec3::ZERO; 4];
        assert_eq!(sphere_span_image(&s, z, z, 1.0), SpanImage::NotASphere);
    }
}
