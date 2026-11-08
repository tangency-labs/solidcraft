//! Whether the two faces of an edge meet tangentially all along it, proved from the faces'
//! NURBS forms and the edge uses' pcurves, and the least angle between them along it.
//!
//! Upstream: none (not in SMLib; SMLib's `IsTangentSector` samples five fixed stations,
//! [`crate::topology::edgeuse::p2::is_tangent_sector`]).
//!
//! | Rust                    | Upstream |
//! |-------------------------|----------|
//! | [`tangent_all_along`]   | (none)   |
//! | [`least_angle_along`], [`LeastAngle`] | (none) |
//! | [`Tangency`]            | (none)   |
//! | `bernstein_roots` (private) | (none) |
//!
//! **The polynomials.** Each use of the edge carries its pcurve `(u(t), v(t)) = (X, Y) / W`
//! on the edge's own parameter `t`, and its face's surface is its NURBS form `A / w` in the
//! form's parameters (every analytic surface is one; a blend is taken as its net). On a piece
//! of `t` where the pcurve stays within one of its knot spans and one knot cell of its net,
//! these are polynomials, of degree `r` and `(p, q)`. The net's natural normal times `w^4` is
//! `N_w = (A_u w - A w_u) x (A_v w - A w_v)`, a polynomial in `(u, v)` of total degree
//! `m = 4p + 4q - 2` (`2p + 2q - 2` where the weights are one value, `w` then constant; 0 for
//! a plane); along the pcurve `W^m N_w` is a polynomial in `t` of degree `m r`. With each use's
//! `F = s W^m N_w`, `s` the sign turning it into the face use's normal, `P = F_a x F_c` and
//! `Q = F_a . F_c` are polynomials in `t` of degree at most the sum `D` of the uses', and the
//! angle between the face use normals is `atan2(|P|, Q)` (`w`, `W` positive).
//!
//! **The values.** `F` at a point `t_j` is evaluated in rounded interval arithmetic
//! ([`Ivl`]): the pcurve's homogeneous `(X, Y, W)` by its B-spline basis, `(u, v)` as a box,
//! the net's homogeneous point and partials over that box (the hull over the knot spans the
//! box meets), then `N_w` and `W^m`. So `P(t_j)` and `Q(t_j)` lie in the intervals computed.
//!
//! **The decision.** On each piece, at `D + 1` Chebyshev points `t_j`, a polynomial of degree
//! `D` is its interpolant, `P(t) = sum_j P(t_j) L_j(t)`: for every unit `e`,
//! `|e . P(t)| <= max_j |P(t_j)| sum_j |L_j(t)|`, so `|P(t)| <= L M` between the points, `M`
//! the largest `|P(t_j)|` enclosed and `L` the Lebesgue constant of the points, under
//! `1 + (2/pi) ln(D + 1)` (Rivlin, *An Introduction to the Approximation of Functions*, 1.4;
//! rounded up). Since the `L_j` sum to 1, `Q(t) >= c - L r` for `c` the middle and `r` the
//! half range of the hull of the `Q(t_j)`. Where that is positive, the faces' angle is at most
//! `atan(L M / (c - L r))` all over the piece. Then:
//! - **not tangent** where at one point the angle's lower end, `atan2` of `|P|`'s lower end and
//!   `Q`'s upper end, less the piece's margin, exceeds the angle asked: a witness;
//! - **tangent** where on every piece the bound plus the piece's margin is within it (exactly
//!   parallel nets, every `P(t_j)` exactly 0 and every `Q(t_j)` positive, give 0);
//! - otherwise the piece is halved, at most `MAX_SPLITS` times, and a piece still neither is
//!   [`Tangency::Undecided`].
//!
//! **Rounded points.** The points `t_j` are the exact Chebyshev points `t*_j` rounded: in the
//! piece's `[-1, 1]` scale `|t_j - t*_j| <= delta` (`node_error`: the angle
//! `(2j + 1) pi / 2n` within `gamma_4 pi`, its cosine within 2 ulps of `f64::cos` (the count
//! taken for the platform's `cos`), and `m + h c` within `gamma_3 (|m| / h + 1)`). By Markov's
//! inequality `|P'| <= D^2 ||P||` on `[-1, 1]`, so `|e . P(t*_j)| <= M + delta D^2 ||P||` for
//! each unit `e`, and the interpolant at the exact points gives
//! `||P|| <= L (M + delta D^2 ||P||)`, that is `||P|| <= L M / (1 - L delta D^2)`, used in
//! place of `L M` (refused, the piece undecided, where `L delta D^2 >= 1`). Likewise
//! `Q(t) >= c - L (r + delta D^2 ||Q||)` with `||Q|| <= L A / (1 - L delta D^2)`, `A` the
//! largest `|Q(t_j)|` enclosed.
//!
//! The pieces' ends are the pcurves' knots and the parameters where a pcurve crosses a knot
//! line of its net, each a root of `U(t) - k W(t)` on one Bezier span of the pcurve, isolated
//! by subdividing its Bernstein coefficients (no sign change: no root; one: exactly one) and
//! found to rounding by bisection. A piece's box that meets two spans takes the hull of both.
//!
//! **The sense.** The two uses are an edge use and its radial partner, the face uses on the
//! two sides of one region. Their face use normals (the surface's natural normal turned by the
//! face use's orientation, as [`evaluate_binormal`] gives it) agree where the faces meet
//! smoothly and are opposite where they fold back on each other (a cusp). The sign `s` is read
//! once per use, at the first point where its `N_w` is proved non-zero, from
//! [`evaluate_binormal`]: a face whose evaluator's normal and its net's are not within 60
//! degrees there is refused.
//!
//! **A blend's net.** A procedural blend's face is decided on its net, the polynomial the proof
//! needs, and its definition is carried as a margin over each piece: the pcurve's `(u, v)` over
//! the piece is enclosed as a box, the definition's `S_u x S_v` over it by
//! [`definition_partials_over_box`] and the net's `N_w` over it as above, and the angle between
//! any two vectors of those enclosures, from `cos >= (D . B)_lo / (|D|_hi |B|_hi)`, is the
//! margin (pi where that is not positive). The margin is the sum of both uses'. A face that is
//! not a blend has margin 0.
//!
//! Deviations: no upstream counterpart. `bernstein_roots` recurses at most `MAX_DEPTH` (48)
//! deep and `MAX_SUBDIVISIONS` (4096) times per call; a piece is halved at most `MAX_SPLITS`
//! (12) times, on an explicit stack; one edge evaluates at most `MAX_POINTS` (one million)
//! points of a cross product of degree at most `MAX_DEGREE` (4096).

use crate::blend::fillet_section::definition_box::{definition_partials_over_box, gn_def_face_of};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{as_b_spline, span, Nurbs};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{CPoint, NOW};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, Brep, EdgeId, EdgeuseId};

const OP: &str = "query::tangent_edge";

/// Most points one edge's decision evaluates.
const MAX_POINTS: usize = 1_000_000;
/// Largest degree of a piece's cross product the decision takes.
const MAX_DEGREE: usize = 4096;
/// Halvings of one piece the decision makes before leaving it undecided.
const MAX_SPLITS: u32 = 12;
/// Subdivisions of one Bernstein polynomial while isolating its roots: its pieces are then
/// 2^-48 of a pcurve span, where a root left unisolated (a pcurve touching a knot line) ends a
/// piece there.
pub(crate) const MAX_DEPTH: u32 = 48;
/// Subdivisions one root isolation may make: a root that touches its knot line (a double
/// root, the only kind that keeps two sign changes as the pieces shrink) costs one a level.
pub(crate) const MAX_SUBDIVISIONS: usize = 4096;
/// The least cosine between a face use's normal from the face's evaluator and from its net
/// at one point: below it the net is not the face's surface and the face is refused.
const SAME_SURFACE_COS: f64 = 0.5;

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// An interval vector.
type I3 = [Ivl; 3];

fn cross(a: I3, b: I3) -> I3 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn dot(a: I3, b: I3) -> Ivl {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    ax * bx + ay * by + az * bz
}

fn scale(a: I3, s: Ivl) -> I3 {
    a.map(|x| x * s)
}

/// `x^2`, never below 0.
fn square(x: Ivl) -> Ivl {
    let s = x * x;
    Ivl::new(s.lo.max(0.0), s.hi)
}

/// `|a|`, enclosed.
fn norm(a: I3) -> Ivl {
    let [x, y, z] = a;
    (square(x) + square(y) + square(z)).sqrt()
}

/// The knot spans `k` (`u[k] < u[k + 1]`, `p <= k < n`) of a knot vector of degree `p` with
/// `n` control points whose interval meets `[lo, hi]`; past the domain, its end span.
fn spans_meeting(u: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> Vec<usize> {
    let mut out: Vec<usize> = (p..n)
        .filter(|&k| match (u.get(k), u.get(k + 1)) {
            (Some(&a), Some(&b)) => a < b && a <= hi && b >= lo,
            _ => false,
        })
        .collect();
    if out.is_empty() {
        out.push(span(u, p, n, 0.5 * (lo + hi)));
    }
    out
}

/// The non-zero basis functions `N_{k-p..=k, p}` over `x` in span `k` and their first
/// derivatives, enclosed (the NURBS book's A2.2 for degrees `p - 1` and `p`;
/// `N' = p (N_{i,p-1} / (U_{i+p} - U_i) - N_{i+1,p-1} / (U_{i+p+1} - U_{i+1}))`).
fn basis(knots: &[f64], k: usize, p: usize, x: Ivl) -> Option<(Vec<Ivl>, Vec<Ivl>)> {
    let u = |i: usize| knots.get(i).copied().map(Ivl::from);
    let mut n = vec![Ivl::from(1.0)];
    let mut lower = Vec::new();
    for j in 1..=p {
        if j == p {
            lower.clone_from(&n);
        }
        let mut next = vec![Ivl::from(0.0); j + 1];
        let mut saved = Ivl::from(0.0);
        for r in 0..j {
            let denom = u(k + 1 + r)? - u((k + 1 + r).checked_sub(j)?)?;
            if !denom.is_positive() {
                return None;
            }
            let temp = *n.get(r)? / denom;
            let right = u(k + 1 + r)? - x;
            let left = x - u((k + 1 + r).checked_sub(j)?)?;
            *next.get_mut(r)? = saved + right * temp;
            saved = left * temp;
        }
        *next.get_mut(j)? = saved;
        n = next;
    }
    let mut d = vec![Ivl::from(0.0); p + 1];
    if p > 0 {
        let pf = Ivl::from(f64::from(u32::try_from(p).ok()?));
        for (j, dj) in d.iter_mut().enumerate() {
            let i = (k + j).checked_sub(p)?;
            let mut v = Ivl::from(0.0);
            if j >= 1 {
                let denom = u(i + p)? - u(i)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v + *lower.get(j - 1)? / denom;
            }
            if j < p {
                let denom = u(i + p + 1)? - u(i + 1)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v - *lower.get(j)? / denom;
            }
            *dj = pf * v;
        }
    }
    Some((n, d))
}

/// One use of the edge: its face's net, its pcurve, the degrees of its normal along a piece,
/// the knot lines of its net and its sense.
struct Use {
    eu: EdgeuseId,
    /// The face's net.
    net: Nurbs,
    /// Whether the net's weights are not all one value.
    rational: bool,
    /// The pcurve's NURBS form (its Bezier spans give the pieces' ends).
    nurb: NurbCurve,
    /// The pcurve's knots, degree and homogeneous control points `(x w, y w, w)`.
    pc_knots: Vec<f64>,
    pc_degree: usize,
    pc_points: Vec<[f64; 3]>,
    /// The degree `m` of the normal in `(u, v)`.
    uv_degree: usize,
    /// The degree of the normal along a piece, `m r`.
    degree: usize,
    /// The net's distinct knots in u and v over its domain: a polynomial cell's sides (none
    /// for a plane, whose normal is constant).
    lines: [Vec<f64>; 2],
    /// The sign `s` turning `N_w` into the face use's normal, once read.
    sign: Option<f64>,
    /// A procedural blend's surface, whose definition the net is measured against.
    definition: Option<Surface>,
}

/// The distinct knots of a knot vector of degree `p` over its domain `[u[p], u[len - p - 1]]`.
fn knot_lines(u: &[f64], p: usize) -> Vec<f64> {
    let end = u.len().saturating_sub(p + 1);
    let mut out: Vec<f64> = u.get(p..=end).map(<[f64]>::to_vec).unwrap_or_default();
    out.dedup();
    out
}

impl Use {
    fn new(k: &Kernel, b: &Brep, eu: EdgeuseId) -> Result<Self> {
        let f = eu1::face(b, eu)?.ok_or_else(|| invalid("an edge use without face"))?;
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| invalid("a face without surface"))?;
        let plane = matches!(surface.kind(), SurfaceKind::Plane(_));
        let definition = matches!(surface.kind(), SurfaceKind::ProceduralBlend(_))
            .then(|| surface.clone_for_evaluation())
            .transpose()?;
        let bs = as_b_spline(k, surface)?
            .ok_or_else(|| unsupported("a face whose surface has no NURBS form"))?;
        let net = Nurbs::of(&bs)?;
        let pcurve = eu1::uv_trim_curve(b, eu)?
            .0
            .ok_or_else(|| unsupported("an edge use without a pcurve"))?;
        let base = pcurve
            .b_spline_base()
            .ok_or_else(|| unsupported("a pcurve with no NURBS form"))?;
        let r = base.degree()?;
        let (unique, mult) = base.knots(None)?;
        let mut knots = Vec::new();
        for (&t, &m) in unique.iter().zip(&mult) {
            knots.extend(std::iter::repeat_n(t, m));
        }
        // the control polygon's weights: none for a non-rational pcurve, else one each
        let (points, weights) = base.control_polygon()?;
        if !weights.is_empty() && weights.len() != points.len() {
            return Err(invalid(
                "a pcurve's weights do not match its control points",
            ));
        }
        let mut pw = Vec::new();
        let mut pc_points = Vec::new();
        for (i, p) in points.iter().enumerate() {
            if weights.is_empty() {
                pw.push(CPoint::new(p.x, p.y, 0.0, NOW));
                pc_points.push([p.x, p.y, 1.0]);
                continue;
            }
            let Some(&w) = weights.get(i) else {
                return Err(invalid(
                    "a pcurve's weights do not match its control points",
                ));
            };
            let positive = w > 0.0;
            if !positive {
                return Err(invalid("a pcurve's weight that is not positive"));
            }
            pw.push(CPoint::new(p.x * w, p.y * w, 0.0, w));
            pc_points.push([p.x * w, p.y * w, w]);
        }
        let nurb = NurbCurve::new(pw, r, knots.clone())?;
        let w: Vec<f64> = net.pw.chunks(4).filter_map(|c| c.get(3).copied()).collect();
        if w.len() != net.nu * net.nv {
            return Err(invalid("a net's weights do not match its control points"));
        }
        let first = w
            .first()
            .copied()
            .ok_or_else(|| invalid("a net without control points"))?;
        let rational = w.iter().any(|&x| x != first);
        let (uv_degree, lines) = if plane {
            (0, [Vec::new(), Vec::new()])
        } else if rational {
            (
                (4 * net.p + 4 * net.q).saturating_sub(2),
                [knot_lines(&net.u, net.p), knot_lines(&net.v, net.q)],
            )
        } else {
            (
                (2 * net.p + 2 * net.q).saturating_sub(2),
                [knot_lines(&net.u, net.p), knot_lines(&net.v, net.q)],
            )
        };
        Ok(Self {
            eu,
            net,
            rational,
            nurb,
            pc_knots: knots,
            pc_degree: r,
            pc_points,
            uv_degree,
            degree: uv_degree.saturating_mul(r),
            lines,
            sign: None,
            definition,
        })
    }

    /// The pcurve's homogeneous `(X, Y, W)` over `t`, enclosed.
    fn pcurve(&self, t: Ivl) -> Result<[Ivl; 3]> {
        let (r, n) = (self.pc_degree, self.pc_points.len());
        let mut out: Option<[Ivl; 3]> = None;
        for kt in spans_meeting(&self.pc_knots, r, n, t.lo, t.hi) {
            let (nb, _) = basis(&self.pc_knots, kt, r, t)
                .ok_or_else(|| invalid("a pcurve's degenerate knots"))?;
            let mut h = [Ivl::from(0.0); 3];
            for (i, bi) in nb.iter().enumerate() {
                let c = (kt + i)
                    .checked_sub(r)
                    .and_then(|j| self.pc_points.get(j))
                    .ok_or_else(|| invalid("a pcurve's control point"))?;
                for (hx, cx) in h.iter_mut().zip(c) {
                    *hx = *hx + *bi * Ivl::from(*cx);
                }
            }
            out = Some(match out {
                None => h,
                Some(o) => [o[0].hull(h[0]), o[1].hull(h[1]), o[2].hull(h[2])],
            });
        }
        out.ok_or_else(|| invalid("a pcurve without a span"))
    }

    /// The `(u, v)` box of the pcurve over `t`, and its `W`.
    fn uv(&self, t: Ivl) -> Result<(Ivl, Ivl, Ivl)> {
        let [x, y, w] = self.pcurve(t)?;
        if !w.is_positive() {
            return Err(invalid("a pcurve's weight not proved positive"));
        }
        Ok((x / w, y / w, w))
    }

    /// The net's `N_w` over the box `u x v` (the hull over the knot cells it meets), enclosed.
    fn n_w(&self, u: Ivl, v: Ivl) -> Result<I3> {
        let n = &self.net;
        let refuse = || invalid("a net's degenerate knots");
        let mut out: Option<I3> = None;
        for ku in spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi) {
            for kv in spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi) {
                let (bu, du) = basis(&n.u, ku, n.p, u).ok_or_else(refuse)?;
                let (bv, dv) = basis(&n.v, kv, n.q, v).ok_or_else(refuse)?;
                let zero = Ivl::from(0.0);
                let (mut h, mut hu, mut hv) = ([zero; 4], [zero; 4], [zero; 4]);
                for (a, (fu, fdu)) in bu.iter().zip(&du).enumerate() {
                    for (c, (fv, fdv)) in bv.iter().zip(&dv).enumerate() {
                        let i = (ku + a).checked_sub(n.p).ok_or_else(refuse)?;
                        let j = (kv + c).checked_sub(n.q).ok_or_else(refuse)?;
                        let at = 4 * (i * n.nv + j);
                        let cp =
                            n.pw.get(at..at + 4)
                                .ok_or_else(|| invalid("a net's control point"))?;
                        let (f, f_u, f_v) = (*fu * *fv, *fdu * *fv, *fu * *fdv);
                        for (l, x) in cp.iter().enumerate() {
                            let x = Ivl::from(*x);
                            if let (Some(hl), Some(hul), Some(hvl)) =
                                (h.get_mut(l), hu.get_mut(l), hv.get_mut(l))
                            {
                                *hl = *hl + f * x;
                                *hul = *hul + f_u * x;
                                *hvl = *hvl + f_v * x;
                            }
                        }
                    }
                }
                let [ax, ay, az, w] = h;
                let [aux, auy, auz, wu] = hu;
                let [avx, avy, avz, wv] = hv;
                let n_w = if self.rational {
                    let tu = [aux * w - ax * wu, auy * w - ay * wu, auz * w - az * wu];
                    let tv = [avx * w - ax * wv, avy * w - ay * wv, avz * w - az * wv];
                    cross(tu, tv)
                } else {
                    // the weights one constant c: A_u x A_v is c^2 S_u x S_v, a positive
                    // multiple of N_w, and of degree 2p + 2q - 2
                    cross([aux, auy, auz], [avx, avy, avz])
                };
                out = Some(match out {
                    None => n_w,
                    Some(o) => [o[0].hull(n_w[0]), o[1].hull(n_w[1]), o[2].hull(n_w[2])],
                });
            }
        }
        out.ok_or_else(|| invalid("a net without a cell"))
    }

    /// `W^m N_w` at `t` (unsigned), enclosed, and its float middle.
    fn raw(&self, t: Ivl) -> Result<I3> {
        let (u, v, w) = self.uv(t)?;
        let n_w = self.n_w(u, v)?;
        let mut wm = Ivl::from(1.0);
        for _ in 0..self.uv_degree {
            wm = wm * w;
        }
        Ok(scale(n_w, wm))
    }

    /// `F = s W^m N_w` at `t`, enclosed.
    fn f(&self, t: f64) -> Result<I3> {
        let s = self
            .sign
            .ok_or_else(|| invalid("a use whose sense is not read"))?;
        Ok(scale(self.raw(Ivl::from(t))?, Ivl::from(s)))
    }

    /// Reads the sign `s` from the face's evaluator at `t`, where `N_w` is proved non-zero;
    /// whether it was read.
    fn read_sign(&mut self, k: &Kernel, b: &mut Brep, t: f64) -> Result<bool> {
        let raw = self.raw(Ivl::from(t))?;
        if !norm(raw).is_positive() {
            return Ok(false);
        }
        let mid = Vec3::new(raw[0].mid(), raw[1].mid(), raw[2].mid());
        let options = BinormalOptions::default();
        let at = evaluate_binormal(k, b, self.eu, t, false, &options)?;
        let fu = at.face_normal;
        let len = fu.length() * mid.length();
        let has_normal = len > 0.0;
        if !has_normal {
            return Err(invalid("a face use without a normal"));
        }
        let c = mid.dot(fu) / len;
        let same_surface = c.abs() >= SAME_SURFACE_COS;
        if !same_surface {
            return Err(unsupported(
                "a face whose evaluator's normal and its NURBS form's disagree",
            ));
        }
        self.sign = Some(c.signum());
        Ok(true)
    }

    /// The angle within which the face's own normal lies of its net's over the piece
    /// `[s0, s1]` (module docs): 0 for a face that is not a blend.
    fn margin(&self, k: &Kernel, s0: f64, s1: f64) -> Result<f64> {
        let Some(def) = &self.definition else {
            return Ok(0.0);
        };
        let (u, v, _) = self.uv(Ivl::new(s0, s1))?;
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, u.mid());
        let kv = span(&n.v, n.q, n.nv, v.mid());
        let unbounded = |e: SolidError| {
            if e.is_unported_or_cancelled() {
                Err(e)
            } else {
                Ok(std::f64::consts::PI)
            }
        };
        let mut d = match gn_def_face_of(k, def) {
            Ok(d) => d,
            Err(e) => return unbounded(e),
        };
        let parts =
            match definition_partials_over_box(k, &mut d, n, ku, kv, u.lo, u.hi, v.lo, v.hi, 1) {
                Ok(p) => p,
                Err(e) => return unbounded(e),
            };
        let (su, sv) = (parts.at(1, 0), parts.at(0, 1));
        let def_n = cross([su.x, su.y, su.z], [sv.x, sv.y, sv.z]);
        let net_n = self.n_w(u, v)?;
        let cos = dot(def_n, net_n) / (norm(def_n) * norm(net_n));
        let proved = cos.lo > 0.0 && cos.lo <= 1.0;
        Ok(if proved {
            cos.lo.acos() * (1.0 + 4.0 * f64::EPSILON)
        } else {
            std::f64::consts::PI
        })
    }

    /// The parameters in `(t0, t1)` where the normal stops being one polynomial: the
    /// pcurve's own knots, and where it crosses a knot line of the net.
    fn breaks(&self, t0: f64, t1: f64, out: &mut Vec<f64>) -> Result<()> {
        out.extend(
            self.nurb
                .knots()
                .iter()
                .copied()
                .filter(|&t| t > t0 && t < t1),
        );
        if self.lines.iter().all(Vec::is_empty) {
            return Ok(());
        }
        for span in self.nurb.decompose_bez()? {
            let (Some(&a), Some(&b)) = (span.knots().first(), span.knots().last()) else {
                continue;
            };
            if !(b > t0 && a < t1) {
                continue;
            }
            let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
            for (d, lines) in self.lines.iter().enumerate() {
                let coord = |c: &CPoint| if d == 0 { c.x } else { c.y };
                // the span's range in this direction: its control points' hull (weights > 0)
                let along: Vec<f64> = span.cpts().iter().map(|c| coord(c) / w(c)).collect();
                let lo = along.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = along.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if !(lo.is_finite() && hi.is_finite()) {
                    return Err(invalid("a pcurve's control points"));
                }
                for &line in lines.iter().filter(|&&l| l >= lo && l <= hi) {
                    // coefficients within rounding of zero are zero: a pcurve running along
                    // the knot line (a seam, a boundary) crosses none there
                    let scale = span
                        .cpts()
                        .iter()
                        .map(|c| coord(c).abs() + (line * w(c)).abs())
                        .fold(0.0, f64::max);
                    let floor = 64.0 * f64::EPSILON * scale;
                    let c: Vec<f64> = span
                        .cpts()
                        .iter()
                        .map(|c| coord(c) - line * w(c))
                        .map(|x| if x.abs() <= floor { 0.0 } else { x })
                        .collect();
                    if c.iter().all(|x| *x == 0.0) {
                        continue;
                    }
                    let mut roots = Vec::new();
                    let mut work = MAX_SUBDIVISIONS;
                    bernstein_roots(&c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut roots);
                    out.extend(roots.into_iter().map(|s| a + (b - a) * s));
                }
            }
        }
        Ok(())
    }
}

/// The value at `s` of the polynomial with Bernstein coefficients `c` on `[0, 1]` (de
/// Casteljau); `None` for no coefficients.
fn bernstein_value(c: &[f64], s: f64) -> Option<f64> {
    let mut b = c.to_vec();
    while b.len() > 1 {
        b = b
            .windows(2)
            .map(|w| {
                if let [x, y] = w {
                    x * (1.0 - s) + y * s
                } else {
                    0.0
                }
            })
            .collect();
    }
    b.first().copied()
}

/// The Bernstein coefficients of the polynomial `c` on `[0, 1/2]` and `[1/2, 1]`.
fn bernstein_halves(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut b = c.to_vec();
    while let (Some(&f), Some(&l)) = (b.first(), b.last()) {
        left.push(f);
        right.push(l);
        b = b
            .windows(2)
            .map(|w| if let [x, y] = w { 0.5 * (x + y) } else { 0.0 })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The roots in `[lo, hi]` of the polynomial with Bernstein coefficients `c` there, as places
/// in `[0, 1]`: none where the coefficients keep one sign, exactly one where they change sign
/// once (found by bisection to rounding), otherwise those of each half. Out of depth or work,
/// the piece's two ends stand for the roots it holds, and the result is `false`: the roots were
/// not all isolated (a root of even multiplicity, or roots closer than the depth resolves).
pub(crate) fn bernstein_roots(
    c: &[f64],
    lo: f64,
    hi: f64,
    depth: u32,
    work: &mut usize,
    out: &mut Vec<f64>,
) -> bool {
    let (Some(&first), Some(&last)) = (c.first(), c.last()) else {
        return true;
    };
    if first == 0.0 {
        out.push(lo);
    }
    if last == 0.0 {
        out.push(hi);
    }
    let signs: Vec<bool> = c.iter().filter(|x| **x != 0.0).map(|x| *x > 0.0).collect();
    let changes = signs
        .windows(2)
        .filter(|w| matches!(w, [x, y] if x != y))
        .count();
    if changes == 0 {
        return true;
    }
    if changes == 1 && first != 0.0 && last != 0.0 {
        let (mut a, mut b) = (0.0, 1.0);
        for _ in 0..64 {
            let m = 0.5 * (a + b);
            if !(m > a && m < b) {
                break;
            }
            let Some(value) = bernstein_value(c, m) else {
                break;
            };
            if (value > 0.0) == (first > 0.0) {
                a = m;
            } else {
                b = m;
            }
        }
        out.push(lo + (hi - lo) * 0.5 * (a + b));
        return true;
    }
    if depth == 0 || *work == 0 {
        out.extend([lo, hi]);
        return false;
    }
    *work -= 1;
    let mid = 0.5 * (lo + hi);
    let (l, r) = bernstein_halves(c);
    let left = bernstein_roots(&l, lo, mid, depth - 1, work, out);
    let right = bernstein_roots(&r, mid, hi, depth - 1, work, out);
    left && right
}

/// The two uses' values at one point: `P` and `Q` (module docs), enclosed, and a lower end of
/// the angle between the face use normals where both are proved non-zero.
struct Node {
    p: I3,
    q: Ivl,
    angle_lo: Option<f64>,
}

/// An edge between two face uses, ready to decide.
struct Pair {
    a: Use,
    c: Use,
    /// `D + 1`.
    nodes: usize,
    /// The Lebesgue constant of `D + 1` Chebyshev points, rounded up.
    lebesgue: f64,
    /// The pieces' ends.
    breaks: Vec<f64>,
    /// Points evaluated so far.
    points: usize,
}

/// The pieces between consecutive `breaks` of some length.
fn pieces(breaks: &[f64]) -> Vec<(f64, f64)> {
    breaks
        .windows(2)
        .filter_map(|w| match *w {
            [s0, s1] if s1 > s0 => Some((s0, s1)),
            _ => None,
        })
        .collect()
}

/// An upper bound of the Lebesgue constant of `n` Chebyshev points, `1 + (2/pi) ln n`
/// (module docs), rounded up.
fn lebesgue(n: usize) -> f64 {
    let bound = 1.0 + 2.0 / std::f64::consts::PI * (n as f64).ln();
    bound * (1.0 + 8.0 * f64::EPSILON)
}

/// `gamma_n = n u / (1 - n u)` (Higham), `u` the unit roundoff `2^-53`.
fn gamma(n: u32) -> f64 {
    let nu = f64::from(n) * (0.5 * f64::EPSILON);
    nu / (1.0 - nu)
}

/// A bound `delta` on how far the rounded Chebyshev points of `[s0, s1]` lie from the exact
/// ones, in the piece's `[-1, 1]` scale (module docs).
fn node_error(s0: f64, s1: f64) -> f64 {
    let (m, h) = (0.5 * (s0 + s1), 0.5 * (s1 - s0));
    let angle = gamma(4) * std::f64::consts::PI;
    let cosine = 2.0 * f64::EPSILON;
    let mapped = gamma(3) * (m.abs() / h + 1.0);
    (angle + cosine + mapped) * (1.0 + gamma(4))
}

/// The bound on the angle between the nets' normals all over a piece from its enclosed values
/// at its rounded Chebyshev points (module docs): `atan(||P|| / Q_floor)`, 0 where every
/// `P(t_j)` is exactly 0 and every `Q(t_j)` positive at exact points; `None` where `Q` is not
/// bounded away from 0 or the points' rounding is too large for the piece (`eta`, the
/// `delta D^2` of the module docs, with `L eta >= 1`).
fn piece_bound(nodes: &[Node], lebesgue: f64, eta: f64) -> Option<f64> {
    let big_m = nodes.iter().map(|n| norm(n.p).hi).fold(0.0, f64::max);
    let q_lo = nodes.iter().map(|n| n.q.lo).fold(f64::INFINITY, f64::min);
    let q_hi = nodes
        .iter()
        .map(|n| n.q.hi)
        .fold(f64::NEG_INFINITY, f64::max);
    let q_abs = nodes.iter().map(|n| n.q.mag()).fold(0.0, f64::max);
    if big_m == 0.0 && q_lo > 0.0 && eta == 0.0 {
        return Some(0.0);
    }
    let lam = Ivl::from(lebesgue);
    let one = Ivl::from(1.0);
    let room = one - lam * Ivl::from(eta);
    if !room.is_positive() {
        return None;
    }
    let p_norm = lam * Ivl::from(big_m) / room;
    let q_norm = lam * Ivl::from(q_abs) / room;
    let (lo, hi) = (Ivl::from(q_lo), Ivl::from(q_hi));
    let half = Ivl::from(0.5);
    let floor = (lo + hi) * half - lam * ((hi - lo) * half + Ivl::from(eta) * q_norm);
    if !floor.is_positive() {
        return None;
    }
    let ratio = p_norm / floor;
    Some(ratio.hi.atan() * (1.0 + 4.0 * f64::EPSILON))
}

/// The Chebyshev points of `[s0, s1]`, `n` of them.
fn chebyshev(s0: f64, s1: f64, n: usize) -> impl Iterator<Item = f64> {
    let (m, h) = (0.5 * (s0 + s1), 0.5 * (s1 - s0));
    (0..n).map(move |j| {
        let angle = (2 * j + 1) as f64 * std::f64::consts::PI / (2 * n) as f64;
        m + h * angle.cos()
    })
}

impl Pair {
    /// The pair of edge `e`, its signs read; `None` for an edge not between two face uses.
    fn of(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<Option<Self>> {
        if !edge::is_manifold(b, e)? || eu1::edge_is_lamina(b, e)? {
            return Ok(None);
        }
        let eu0 = eu1::primary_edgeuse(b, e)?.ok_or_else(|| invalid("an edge without edge use"))?;
        let Some(eu_r) = eu1::radial(b, eu0)? else {
            return Ok(None);
        };
        let a = Use::new(k, b, eu0)?;
        let c = Use::new(k, b, eu_r)?;
        let degree = a.degree.saturating_add(c.degree);
        if degree > MAX_DEGREE {
            return Err(unsupported(
                "an edge whose faces' normals are of too high a degree along it",
            ));
        }
        let ivl = edge::interval(b, e)?;
        let (t0, t1) = (ivl.min(), ivl.max());
        let mut breaks = vec![t0, t1];
        a.breaks(t0, t1, &mut breaks)?;
        c.breaks(t0, t1, &mut breaks)?;
        breaks.retain(|t| *t >= t0 && *t <= t1);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let nodes = degree + 1;
        let mut pair = Self {
            a,
            c,
            nodes,
            lebesgue: lebesgue(nodes),
            breaks,
            points: 0,
        };
        // the signs, at the first points where each use's N_w is proved non-zero
        'signs: for (s0, s1) in pieces(&pair.breaks) {
            for t in chebyshev(s0, s1, nodes) {
                if pair.a.sign.is_none() {
                    pair.a.read_sign(k, b, t)?;
                }
                if pair.c.sign.is_none() {
                    pair.c.read_sign(k, b, t)?;
                }
                if pair.a.sign.is_some() && pair.c.sign.is_some() {
                    break 'signs;
                }
            }
        }
        if pair.a.sign.is_none() || pair.c.sign.is_none() {
            return Err(invalid(
                "an edge whose faces' normals are not proved non-zero anywhere along it",
            ));
        }
        Ok(Some(pair))
    }

    /// The values at `t`.
    fn node(&mut self, t: f64) -> Result<Node> {
        self.points += 1;
        if self.points > MAX_POINTS {
            return Err(unsupported("an edge too costly to decide"));
        }
        let (fa, fc) = (self.a.f(t)?, self.c.f(t)?);
        let p = cross(fa, fc);
        let q = dot(fa, fc);
        let defined = norm(fa).is_positive() && norm(fc).is_positive();
        let angle_lo = defined.then(|| {
            // atan2(y, x) grows with y >= 0 and falls with x: its lower end at (|P|_lo, Q_hi)
            let a = norm(p).lo.atan2(q.hi);
            a - 4.0 * f64::EPSILON * a.abs()
        });
        Ok(Node { p, q, angle_lo })
    }

    /// The margin over the piece `[s0, s1]`: both uses'.
    fn margin(&self, k: &Kernel, s0: f64, s1: f64) -> Result<f64> {
        let m = self.a.margin(k, s0, s1)? + self.c.margin(k, s0, s1)?;
        Ok(m * (1.0 + 2.0 * f64::EPSILON))
    }
}

/// The answer of [`tangent_all_along`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tangency {
    /// The faces meet within the angle asked all along the edge.
    Tangent,
    /// They part by more than the angle asked somewhere along it.
    NotTangent,
    /// A piece of the edge the bound cannot place on either side of the angle asked, even
    /// halved `MAX_SPLITS` times (module docs): `angle` is the bound there (pi where `Q` is
    /// not bounded away from 0) and `margin` the piece's margin.
    Undecided {
        /// The bound on the angle between the nets' normals there (radians).
        angle: f64,
        /// The margin there (radians).
        margin: f64,
    },
}

/// Whether edge `e`'s two face uses meet with normals within `max_angle` (radians) of each
/// other all along it: a smooth edge (a seam a face meets itself across is one). An edge
/// whose faces are tangent at one end only is a crease everywhere else, which is not smooth;
/// nor is a cusp, its faces folded back on each other. `None` for an edge not between two
/// face uses. See the module documentation.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an angle not finite in `[0, pi)` or an edge whose faces'
/// normals are not proved non-zero anywhere along it; [`SolidError::Unsupported`] for a face
/// with no NURBS form, an edge use with no pcurve in NURBS form, or an edge too costly to
/// decide; those of the evaluations.
pub fn tangent_all_along(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    max_angle: f64,
) -> Result<Option<Tangency>> {
    if !(0.0..std::f64::consts::PI).contains(&max_angle) {
        return Err(invalid("a tangency angle in [0, pi)"));
    }
    let Some(mut pair) = Pair::of(k, b, e)? else {
        return Ok(None);
    };
    let mut undecided: Option<Tangency> = None;
    let mut stack: Vec<(f64, f64, u32)> = pieces(&pair.breaks)
        .into_iter()
        .rev()
        .map(|(s0, s1)| (s0, s1, 0))
        .collect();
    while let Some((s0, s1, splits)) = stack.pop() {
        let margin = pair.margin(k, s0, s1)?;
        let mut nodes = Vec::new();
        for t in chebyshev(s0, s1, pair.nodes) {
            nodes.push(pair.node(t)?);
        }
        // a witness
        if nodes
            .iter()
            .filter_map(|n| n.angle_lo)
            .any(|angle| angle - margin > max_angle)
        {
            return Ok(Some(Tangency::NotTangent));
        }
        let degree = (pair.nodes - 1) as f64;
        let eta = node_error(s0, s1) * degree * degree * (1.0 + gamma(2));
        match piece_bound(&nodes, pair.lebesgue, eta) {
            Some(angle) if angle + margin <= max_angle => {}
            _ if splits < MAX_SPLITS => {
                let mid = 0.5 * (s0 + s1);
                stack.push((mid, s1, splits + 1));
                stack.push((s0, mid, splits + 1));
            }
            bound => {
                if undecided.is_none() {
                    undecided = Some(Tangency::Undecided {
                        angle: bound.unwrap_or(std::f64::consts::PI),
                        margin,
                    });
                }
            }
        }
    }
    Ok(Some(undecided.unwrap_or(Tangency::Tangent)))
}

/// The least angle between edge `e`'s faces ([`least_angle_along`]): at the point where the
/// nets' normals are nearest, and the margin of its piece (0 unless a face is a blend).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeastAngle {
    /// The least angle between the nets' normals at the points (radians, 0 to pi): the
    /// middle of its enclosure.
    pub angle: f64,
    /// The margin of that point's piece (radians; module docs).
    pub margin: f64,
}

/// The least angle between the normals of edge `e`'s two face uses at the `D + 1` Chebyshev
/// points of each piece (module docs) where both are proved non-zero: the edge's least angle
/// where it is the same all along it (two planes, a plane and a cylinder along a generator),
/// at most a little above it elsewhere. `None` for an edge not between two face uses.
///
/// # Errors
///
/// As [`tangent_all_along`], but for the angle.
pub fn least_angle_along(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<Option<LeastAngle>> {
    let Some(mut pair) = Pair::of(k, b, e)? else {
        return Ok(None);
    };
    let mut least = LeastAngle {
        angle: f64::INFINITY,
        margin: 0.0,
    };
    for (s0, s1) in pieces(&pair.breaks) {
        let mut piece_least = f64::INFINITY;
        for t in chebyshev(s0, s1, pair.nodes) {
            let n = pair.node(t)?;
            if n.angle_lo.is_some() {
                piece_least = piece_least.min(norm(n.p).mid().atan2(n.q.mid()));
            }
        }
        if piece_least < least.angle {
            least = LeastAngle {
                angle: piece_least,
                margin: pair.margin(k, s0, s1)?,
            };
        }
    }
    Ok(least.angle.is_finite().then_some(least))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i3(v: [f64; 3]) -> I3 {
        v.map(Ivl::from)
    }

    #[test]
    fn bernstein_roots_are_isolated_and_exact() {
        // (s - 0.25)(s - 0.75) = s^2 - s + 3/16, in Bernstein form on [0, 1]:
        // 3/16, 3/16 - 1/2, 3/16
        let c = [0.1875, 0.1875 - 0.5, 0.1875];
        let mut roots = Vec::new();
        let mut work = MAX_SUBDIVISIONS;
        assert!(bernstein_roots(
            &c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut roots
        ));
        roots.sort_by(f64::total_cmp);
        let want = [0.25, 0.75];
        assert_eq!(roots.len(), want.len(), "{roots:?}");
        assert!(
            roots.iter().zip(want).all(|(r, w)| (r - w).abs() < 1e-15),
            "{roots:?}"
        );
        let mut none = Vec::new();
        assert!(bernstein_roots(
            &[1.0, 0.5, 2.0],
            0.0,
            1.0,
            MAX_DEPTH,
            &mut work,
            &mut none
        ));
        assert!(none.is_empty());
        // (s - 1/2)^2 = s^2 - s + 1/4: a double root, never isolated by a sign change; its
        // coefficients halve to exact zeros at 1/2, both halves' ends there
        let mut touch = Vec::new();
        bernstein_roots(
            &[0.25, -0.25, 0.25],
            0.0,
            1.0,
            MAX_DEPTH,
            &mut work,
            &mut touch,
        );
        assert!(touch.iter().all(|&x| x == 0.5), "{touch:?}");
    }

    /// `P(t) = -1 + (8/3) t^2` along one direction, `Q = 1`: at the 3 Chebyshev points of
    /// `[-1, 1]` (0 and +-sqrt(3)/2) `|P|` is 1, an angle of 45 degrees, but at `t = 1` it is
    /// 5/3, 59.04 degrees. Decided at 50 degrees from the points alone this was "tangent"; the
    /// bound `atan(L)`, `L` = 1.70, is 59.5 degrees, over 50, so it is not.
    #[test]
    fn a_piece_s_bound_covers_the_angle_between_its_points() {
        let n = 3;
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| {
                let p = -1.0 + 8.0 / 3.0 * t * t;
                Node {
                    p: i3([p, 0.0, 0.0]),
                    q: Ivl::from(1.0),
                    angle_lo: Some(p.abs().atan2(1.0)),
                }
            })
            .collect();
        let at_points = nodes.iter().filter_map(|n| n.angle_lo).fold(0.0, f64::max);
        assert!(
            (at_points - std::f64::consts::FRAC_PI_4).abs() < 1e-12,
            "{at_points}"
        );
        let between = (5.0_f64 / 3.0).atan();
        let bound = piece_bound(&nodes, lebesgue(n), 0.0).unwrap();
        assert!(bound >= between, "{bound} under {between}");
        assert!(bound > 50.0_f64.to_radians(), "{bound}");
    }

    #[test]
    fn knot_lines_are_the_domain_s_distinct_knots() {
        assert_eq!(
            knot_lines(&[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], 2),
            vec![0.0, 0.5, 1.0]
        );
    }
}
