//! Certificates that a B-rep's UV trims lie on their edges (tests only): for an edge use
//! with a UV trim curve `T` on its face's surface `S`, and its edge's curve `C` over the
//! edge's interval `[s0, s1]`, an upper bound `B` on
//!
//! * the distance from every point `S(T(t))` of the trim's image to the edge's curve, and
//! * the distance from every point of the edge's curve to the trim's image (so to `S`),
//!   but for an end stretch that lies, with the trim's end, inside a vertex's tolerance
//!   ball (certified against that ball instead and reported in `at_vertex`; see below).
//!
//! `check_brep` level 2 checks a trim's owner, length, interval and periodic jumps, not
//! where it lies; this is that check.
//!
//! **The bound.** The trim's parameter range is cut into pieces `[a, b]`; on each, `sigma`
//! is the affine map from `[a, b]` onto `[sa, sb]`, the edge parameters of the pieces' ends
//! (at every end, the trim's own ends too, the parameter whose curve point is nearest to
//! the trim's point, found by Newton's method from the matching edge end or the affine
//! guess: any value is sound, a good one is tight). With `D(t) = S(T(t)) - C(sigma(t))` and `m`
//! the piece's middle, the mean-value inequality for the continuous, piecewise `C^1` `D`
//! gives, for every `t` in `[a, b]`,
//!
//! `|D(t)| <= |D(m)| + sup |D'| (b - a) / 2`, `D' = S_u T_u' + S_v T_v' - alpha C'(sigma)`,
//!
//! `alpha` the slope of `sigma`. Every factor is enclosed in rounded interval arithmetic
//! from the curves' and the surface's NURBS data ([`curve_derivs_over`] over the piece,
//! [`nurbs_partials_over_piece`] over the box of the trim's UV values on it, cut at the
//! knot lines it crosses and hulled; a part past the knots on the end piece continued, as
//! [`Nurbs`] reads a surface there, the distance past reported): `T(m)` and `S` at it, `C(sigma(m))`, `T'` and `S`'s first
//! partials over the piece and `C'` over `sigma`'s range. A piece whose bound is above the
//! target is halved, to a depth of [`MAX_DEPTH`] (a bound left above the target is
//! reported, never dropped). `sigma`'s ranges are cut to `[s0, s1]` (a point of the trim is
//! then within `|D|` of a point of the edge); the true images of two consecutive pieces'
//! `sigma` meet up to the enclosures of their shared end, so every edge parameter lies in
//! some piece's image up to that gap `g`, and an edge point is within `|D| + sup |C'| g` of
//! the trim's image. The pieces cover the edge between the feet `f` of the trim's two ends;
//! a point `C(s)` between a foot and its edge end `e` is bounded by its distance to the
//! trim's end point `P = S(T(t_end))`: on parts `[a, b]` of that stretch,
//! `|C(s) - P| <= |C(m) - P| + sup |C'| max(m - a, b - m)`, `P`, `C(m)` and `C'` enclosed,
//! the parts halved as the pieces are (a trim end off the edge end along the curve then
//! costs that offset once; pinning `sigma` to `e` would make the end piece pay it twice).
//! The certificate's `bound` is the largest of these. A stretch farther than the edge's
//! tolerance from `P` is still accounted for when the stretch and `P` both lie in the
//! tolerance ball of a vertex of the edge (`|C(s) - V|` bounded alike, `|P - V|` enclosed):
//! the vertex joins the edge's end and the trim's there, as a tolerant B-rep's vertex ball
//! does; the certificate's `at_vertex` records the most of those distances over the vertex's
//! tolerance, and the stretch adds nothing to `bound`.
//!
//! The surface is read in its NURBS form (`as_b_spline`, as every analytic surface but the
//! plane is evaluated; the plane's net is its exact bilinear form), a curve-bounded surface
//! as its basis ([`crate::certify::measured`]); the curves in theirs
//! ([`Curve::b_spline_base`], a line's [`Line::base_with_nurb`]).
//!
//! **An edge curve on a surface** (`CrvOnSurf`, [`EdgeCurve::OnSurface`]) is
//! `C(s) = S_c(U(s))`, its UV curve `U` on its own surface `S_c`, as the kernel evaluates
//! it: `C` and `C' = S_c,u U_u' + S_c,v U_v'` are enclosed by the same means, `U` and `U'`
//! over `s` from `U`'s NURBS data, then `S_c` and its partials over the box of `U`'s values
//! (cut at its knot lines and hulled, as for `S`). No point of either is sampled.

use solidcore::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_over_piece, NurbsCurve,
};
use solidcore::blend::fillet_section::interval::{round_up, Ivl};
use solidcore::blend::fillet_section::series::V3;
use solidcore::blend::fillet_section::station::{as_b_spline, Nurbs};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::kernel::Kernel;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeId, EdgeuseId};
use std::cell::Cell;

/// The deepest a piece is halved.
pub(crate) const MAX_DEPTH: u32 = 30;
/// The most pieces one trim is cut into.
const MAX_PIECES: usize = 1 << 18;

/// One edge use's trim certified against its edge.
#[derive(Debug, Clone)]
pub(crate) struct TrimCertificate {
    /// The edge use whose own slot holds the trim.
    pub(crate) edgeuse: EdgeuseId,
    /// Its edge.
    pub(crate) edge: EdgeId,
    /// The edge's tolerance.
    pub(crate) tolerance: f64,
    /// The certified two-sided bound (see the module docs).
    pub(crate) bound: f64,
    /// The pieces the trim was cut into.
    pub(crate) pieces: usize,
    /// Whether the trim runs against the edge's parameter.
    pub(crate) reversed: bool,
    /// The farthest the trim's UV values (or a curve-on-surface edge's) reach past their
    /// surface's knots (0: inside); there the surface is its end piece continued.
    pub(crate) past: f64,
    /// The largest `|D(m)|` enclosed at a piece's midpoint: the bound's lower part, near
    /// the distance the trim actually lies from its edge.
    pub(crate) sampled: f64,
    /// 0, or, when an end stretch of the edge (between an edge end and the foot of the
    /// trim's matching end) is not within the tolerance of the trim's end point but both lie
    /// in a vertex's tolerance ball, the most of their enclosed distances to that vertex over
    /// its tolerance (at most 1; see the module docs).
    pub(crate) at_vertex: f64,
}

impl TrimCertificate {
    /// Whether the bound is within the edge's tolerance.
    pub(crate) fn holds(&self) -> bool {
        self.bound <= self.tolerance
    }
}

/// A curve's NURBS data: its B-spline base, a line's built.
///
/// # Errors
///
/// A kind with no B-spline base, or data that do not agree.
pub(crate) fn curve_nurbs(c: &Curve) -> Result<NurbsCurve, String> {
    let e = |x: solidcore::SolidError| x.to_string();
    match c.kind() {
        CurveKind::Line(l) => {
            let base = l.base_with_nurb().map_err(e)?;
            NurbsCurve::of(base.as_ref()).map_err(e)
        }
        _ => NurbsCurve::of(
            c.b_spline_base()
                .ok_or_else(|| format!("a curve (type {}) with no B-spline base", c.type_code()))?,
        )
        .map_err(e),
    }
}

/// An edge's curve as the certificate reads it.
pub(crate) enum EdgeCurve {
    /// Its NURBS data ([`curve_nurbs`]).
    Nurbs(NurbsCurve),
    /// A curve on a surface (`CrvOnSurf`): the surface's NURBS form at its UV curve's points.
    OnSurface {
        /// The UV curve's NURBS data.
        uv: NurbsCurve,
        /// The surface's NURBS form.
        surface: Nurbs,
    },
}

/// An edge curve's reading ([`EdgeCurve`]): a curve on a surface as its UV curve and its
/// surface's NURBS form ([`crate::certify::measured`]), any other kind as [`curve_nurbs`].
///
/// # Errors
///
/// Those of [`curve_nurbs`]; a surface with no NURBS form.
pub(crate) fn edge_curve(k: &Kernel, c: &Curve) -> Result<EdgeCurve, String> {
    let e = |x: solidcore::SolidError| x.to_string();
    match c.kind() {
        CurveKind::CrvOnSurf(cs) => {
            let surface = crate::certify::measured(cs.surface());
            let form = as_b_spline(k, surface)
                .map_err(e)?
                .ok_or("a curve on a surface with no NURBS form")?;
            Ok(EdgeCurve::OnSurface {
                uv: curve_nurbs(cs.uv_curve())?,
                surface: Nurbs::of(&form).map_err(e)?,
            })
        }
        _ => Ok(EdgeCurve::Nurbs(curve_nurbs(c)?)),
    }
}

fn knot(u: &[f64], i: usize) -> f64 {
    u.get(i).copied().unwrap_or(f64::NAN)
}

/// A NURBS curve's parameter range.
pub(crate) fn range(c: &NurbsCurve) -> (f64, f64) {
    (knot(&c.u, c.p), knot(&c.u, c.n))
}

fn sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn scale(s: Ivl, a: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: s * a.x,
        y: s * a.y,
        z: s * a.z,
    }
}

fn add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// An upper bound on the length of every vector in `a`.
pub(crate) fn norm_hi(a: V3<Ivl>) -> f64 {
    let sq = Ivl::from(a.x.mag()) * Ivl::from(a.x.mag())
        + Ivl::from(a.y.mag()) * Ivl::from(a.y.mag())
        + Ivl::from(a.z.mag()) * Ivl::from(a.z.mag());
    let r = sq.sqrt().hi;
    if r.is_nan() {
        f64::INFINITY
    } else {
        r
    }
}

fn mid(a: V3<Ivl>) -> [f64; 3] {
    [a.x.mid(), a.y.mid(), a.z.mid()]
}

/// The certificate's reading of a trim on a surface against a curve.
struct Pieces<'a> {
    trim: &'a NurbsCurve,
    surface: &'a Nurbs,
    curve: &'a EdgeCurve,
    /// The edge's interval.
    s0: f64,
    s1: f64,
    /// The farthest a UV box reached past the surface's knots (the face's surface, or an
    /// edge curve's own surface).
    past: Cell<f64>,
}

fn v_hull(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x.hull(b.x),
        y: a.y.hull(b.y),
        z: a.z.hull(b.z),
    }
}

/// `[lo, hi]` cut at the distinct knots strictly inside it, each part with its span: a part
/// before the first knot of the range on the first span, after the last on the last.
///
/// A part's span is read from its ends, never from a computed middle (which can round onto
/// the knot at its far end and pick the polynomial on the other side): no knot lies strictly
/// inside a part, so the span holding its start holds all of it. A point part on an interior
/// knot lies on both spans that meet there, and is listed on each.
fn cuts(u: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> Vec<(f64, f64, usize)> {
    let (first, last) = (knot(u, p), knot(u, n));
    // the span with knot(k) <= x < knot(k + 1)
    let span = |x: f64| {
        let mut k = p;
        while k + 1 < n && knot(u, k + 1) <= x {
            k += 1;
        }
        k
    };
    // the span with knot(k) < x <= knot(k + 1)
    let span_left = |x: f64| {
        let mut k = p;
        while k + 1 < n && knot(u, k + 1) < x {
            k += 1;
        }
        k
    };
    let mut at = vec![lo];
    for &x in u {
        if x > lo && x < hi && x >= first && x <= last && at.last() != Some(&x) {
            at.push(x);
        }
    }
    at.push(hi);
    let mut out = Vec::new();
    for (&a, &b) in at.iter().zip(at.iter().skip(1)) {
        let k = span(a);
        out.push((a, b, k));
        if a == b {
            let left = span_left(a);
            if left != k {
                out.push((a, b, left));
            }
        }
    }
    out
}

/// A surface `n` and its first partials over the UV box `uv`: the box cut at the knot
/// lines inside it, each part on its own polynomial piece, a part past the knots on the
/// end piece continued ([`Nurbs`]'s continuation, the distance past kept in `past_out`),
/// and the parts' hull.
fn surface_over(n: &Nurbs, uv: V3<Ivl>, past_out: &Cell<f64>) -> Result<[V3<Ivl>; 3], String> {
    let us = cuts(&n.u, n.p, n.nu, uv.x.lo, uv.x.hi);
    let vs = cuts(&n.v, n.q, n.nv, uv.y.lo, uv.y.hi);
    let mut out: Option<[V3<Ivl>; 3]> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = nurbs_partials_over_piece(n, ku, kv, a0, a1, b0, b1, 1)
                .map_err(|e| e.to_string())?;
            let q = [p.at(0, 0), p.at(1, 0), p.at(0, 1)];
            out = Some(match out {
                None => q,
                Some(h) => [v_hull(h[0], q[0]), v_hull(h[1], q[1]), v_hull(h[2], q[2])],
            });
        }
    }
    let past = (knot(&n.u, n.p) - uv.x.lo)
        .max(uv.x.hi - knot(&n.u, n.nu))
        .max(knot(&n.v, n.q) - uv.y.lo)
        .max(uv.y.hi - knot(&n.v, n.nv))
        .max(0.0);
    past_out.set(past_out.get().max(past));
    out.ok_or_else(|| "no piece encloses the box".to_string())
}

impl Pieces<'_> {
    /// `T` and `T'` over `t`.
    fn trim_over(&self, t: Ivl) -> Result<(V3<Ivl>, V3<Ivl>), String> {
        let d = curve_derivs_over(self.trim, t, 1, None).map_err(|e| e.to_string())?;
        Ok((d[0], d[1]))
    }

    /// `S` and its first partials over the UV box `uv` ([`surface_over`]).
    fn surface_over(&self, uv: V3<Ivl>) -> Result<[V3<Ivl>; 3], String> {
        surface_over(self.surface, uv, &self.past)
    }

    /// `C` and `C'` over `s`: a curve on a surface as `S_c(U(s))` and
    /// `S_c,u U_u' + S_c,v U_v'` (see the module docs).
    fn curve_over(&self, s: Ivl) -> Result<(V3<Ivl>, V3<Ivl>), String> {
        match self.curve {
            EdgeCurve::Nurbs(c) => {
                let d = curve_derivs_over(c, s, 1, None).map_err(|e| e.to_string())?;
                Ok((d[0], d[1]))
            }
            EdgeCurve::OnSurface { uv, surface } => {
                let d = curve_derivs_over(uv, s, 1, None).map_err(|e| e.to_string())?;
                let [p, su, sv] = surface_over(surface, d[0], &self.past)?;
                Ok((p, add(scale(d[1].x, su), scale(d[1].y, sv))))
            }
        }
    }

    /// `C`, `C'` and `C''` at `s` (midpoints; `C''` of a curve on a surface taken as 0, a
    /// Gauss-Newton step): only for [`Pieces::foot`], where any value is sound.
    fn jet(&self, s: f64) -> Option<[[f64; 3]; 3]> {
        match self.curve {
            EdgeCurve::Nurbs(c) => {
                let d = curve_derivs_over(c, Ivl::from(s), 2, None).ok()?;
                Some([mid(d[0]), mid(d[1]), mid(d[2])])
            }
            EdgeCurve::OnSurface { .. } => {
                let (c, c1) = self.curve_over(Ivl::from(s)).ok()?;
                Some([mid(c), mid(c1), [0.0; 3]])
            }
        }
    }

    /// The trim's image point at `t` (the middle of its enclosure).
    fn image(&self, t: f64) -> Result<[f64; 3], String> {
        let (uv, _) = self.trim_over(Ivl::from(t))?;
        Ok(mid(self.surface_over(uv)?[0]))
    }

    /// The edge parameter whose curve point is nearest to `p`, from `s` (Newton's method,
    /// kept in `[s0, s1]`): only `sigma`'s nodes, any value sound.
    fn foot(&self, p: [f64; 3], mut s: f64) -> f64 {
        for _ in 0..16 {
            let Some([c, c1, c2]) = self.jet(s) else {
                break;
            };
            let r = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
            let f = r[0] * c1[0] + r[1] * c1[1] + r[2] * c1[2];
            let df = c1[0] * c1[0]
                + c1[1] * c1[1]
                + c1[2] * c1[2]
                + r[0] * c2[0]
                + r[1] * c2[1]
                + r[2] * c2[2];
            // (a NaN stops too)
            if df.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                break;
            }
            let next = (s - f / df).clamp(self.s0, self.s1);
            if next == s {
                break;
            }
            s = next;
        }
        s
    }

    /// The bound over one piece `[a, b]` with `sigma(a) = sa`, `sigma(b) = sb`: `|D|`'s
    /// bound, `|D(m)|`'s and `sigma`'s range enclosed (before the cut to `[s0, s1]`).
    fn piece(&self, a: f64, b: f64, sa: f64, sb: f64) -> Result<(f64, f64, Ivl), String> {
        let m = 0.5 * (a + b);
        let (ia, ib, im) = (Ivl::from(a), Ivl::from(b), Ivl::from(m));
        // sigma(t) = sa + alpha (t - a), alpha = (sb - sa) / (b - a), in intervals
        let alpha = (Ivl::from(sb) - Ivl::from(sa)) / (ib - ia);
        let sigma_m = Ivl::from(sa) + alpha * (im - ia);
        let sigma = Ivl::from(sa) + alpha * (Ivl::new(a, b) - ia);
        // the part of sigma's range inside the edge: C is evaluated there; a point of sigma
        // outside is within sup |C'| of its cut, added below
        let cut = Ivl::new(sigma.lo.max(self.s0), sigma.hi.min(self.s1));
        let cut_m = Ivl::new(sigma_m.lo.max(self.s0), sigma_m.hi.min(self.s1));
        if !(cut.lo <= cut.hi && cut_m.lo <= cut_m.hi) {
            return Err("sigma leaves the edge's interval".to_string());
        }
        let past = (self.s0 - sigma.lo).max(sigma.hi - self.s1).max(0.0);
        // D(m)
        let (uv_m, _) = self.trim_over(im)?;
        let p_m = self.surface_over(uv_m)?[0];
        let (c_m, _) = self.curve_over(cut_m)?;
        let d_m = norm_hi(sub(p_m, c_m));
        // D' over the piece
        let (uv, t1) = self.trim_over(Ivl::new(a, b))?;
        let [_, su, sv] = self.surface_over(uv)?;
        let p1 = add(scale(t1.x, su), scale(t1.y, sv));
        let (_, c1) = self.curve_over(cut)?;
        let d1 = norm_hi(sub(p1, scale(alpha, c1)));
        let c1_hi = norm_hi(c1);
        // the farthest a point of [a, b] lies from the computed middle `m` (which can round
        // onto an end of a piece two floats wide), as `farthest` takes it
        let half = (im - ia).hi.max((ib - im).hi);
        let bound =
            round_up(round_up(d_m + round_up(d1 * half)) + round_up(c1_hi * round_up(past)));
        Ok((bound, d_m, sigma))
    }
}

/// Certifies the trim in `eu`'s own slot against its edge: halving pieces whose bound is
/// above `target` and more than a quarter above their `|D(m)|` (to [`MAX_DEPTH`]).
///
/// # Errors
///
/// A missing trim, face, surface, edge or curve; data with no NURBS form; an enclosure
/// that cannot be made (a box past a surface's knots).
pub(crate) fn certify_edgeuse(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    target: f64,
) -> Result<TrimCertificate, String> {
    let e = |x: solidcore::SolidError| x.to_string();
    let rec = b.edgeuse(eu).map_err(e)?;
    let trim = rec
        .uv_trim_curve
        .as_ref()
        .ok_or("no trim in the edge use's slot")?;
    let edge = rec.edge.ok_or("no edge")?;
    let face = eu1::face(b, eu).map_err(e)?.ok_or("no face")?;
    let surface = b
        .face(face)
        .map_err(e)?
        .surface
        .as_ref()
        .ok_or("no surface")?;
    let surface = crate::certify::measured(surface);
    let form = as_b_spline(k, surface).map_err(e)?.ok_or("no NURBS form")?;
    let sn = Nurbs::of(&form).map_err(e)?;
    let erec = b.edge(edge).map_err(e)?;
    let curve = erec.curve.as_ref().ok_or("no edge curve")?;
    let cn = edge_curve(k, curve)?;
    let tn = curve_nurbs(trim)?;
    let (t0, t1) = range(&tn);
    let pieces = Pieces {
        trim: &tn,
        surface: &sn,
        curve: &cn,
        s0: erec.interval.min(),
        s1: erec.interval.max(),
        past: Cell::new(0.0),
    };
    // which end of the edge the trim starts at
    let start = pieces.image(t0)?;
    let dist = |s: f64| -> Result<f64, String> {
        let (c, _) = pieces.curve_over(Ivl::from(s))?;
        let c = mid(c);
        Ok(
            ((c[0] - start[0]).powi(2) + (c[1] - start[1]).powi(2) + (c[2] - start[2]).powi(2))
                .sqrt(),
        )
    };
    let (to_s0, to_s1) = (dist(pieces.s0)?, dist(pieces.s1)?);
    let reversed = if (to_s0 - to_s1).abs() <= erec.tolerance {
        // a closed edge (both ends one point): read the direction a quarter along
        let q = pieces.image(t0 + 0.25 * (t1 - t0))?;
        let near = |s: f64| -> Result<f64, String> {
            let c = mid(pieces.curve_over(Ivl::from(s))?.0);
            Ok(((c[0] - q[0]).powi(2) + (c[1] - q[1]).powi(2) + (c[2] - q[2]).powi(2)).sqrt())
        };
        let quarter = 0.25 * (pieces.s1 - pieces.s0);
        near(pieces.s1 - quarter)? < near(pieces.s0 + quarter)?
    } else {
        to_s1 < to_s0
    };
    let (e0, e1) = if reversed {
        (pieces.s1, pieces.s0)
    } else {
        (pieces.s0, pieces.s1)
    };
    // first nodes: the trim's knots, each span halved
    let mut nodes: Vec<f64> = vec![t0];
    let mut prev = t0;
    for &x in &tn.u {
        if x > prev && x < t1 {
            nodes.push(0.5 * (prev + x));
            nodes.push(x);
            prev = x;
        }
    }
    nodes.push(0.5 * (prev + t1));
    nodes.push(t1);
    // every node's edge parameter is the foot of its image, the trim's ends' too (from the
    // edge end they meet): an end piece pinned to the edge end would have to stretch
    // `sigma` over the distance between the trim's end and the edge's end within itself,
    // and its slope term would double that distance; the stretch from a foot to its edge
    // end is covered on its own below
    let mut s_nodes = Vec::with_capacity(nodes.len());
    for &t in &nodes {
        let guess = e0 + (e1 - e0) * (t - t0) / (t1 - t0);
        s_nodes.push(pieces.foot(pieces.image(t)?, guess));
    }
    let (foot0, foot1) = (
        s_nodes.first().copied().unwrap_or(e0),
        s_nodes.last().copied().unwrap_or(e1),
    );
    // an upper bound on the distance from the edge's points over [lo, hi] to the point
    // enclosure `p`: on parts [a, b], |C(s) - p| <= |C(m) - p| + sup |C'| max(m - a, b - m),
    // the parts halved as the trim's pieces are (against the tolerance `tol`)
    let farthest = |p: V3<Ivl>, lo: f64, hi: f64, tol: f64| -> Result<f64, String> {
        let mut parts: Vec<(f64, f64, u32)> = vec![(lo, hi, 0)];
        let (mut worst, mut count) = (0.0_f64, 0usize);
        while let Some((a, b, depth)) = parts.pop() {
            let m = 0.5 * (a + b);
            let (c, _) = pieces.curve_over(Ivl::from(m))?;
            let d_m = norm_hi(sub(p, c));
            let (_, c1) = pieces.curve_over(Ivl::new(a, b))?;
            let half = (Ivl::from(m) - Ivl::from(a))
                .hi
                .max((Ivl::from(b) - Ivl::from(m)).hi);
            let d = round_up(d_m + round_up(norm_hi(c1) * half));
            // halved while the slope term counts, or while a bound above the tolerance may
            // still come under it
            let open = d > 1.25 * d_m || (d > tol && d_m <= tol);
            if d > 0.125 * tol && open && depth < MAX_DEPTH && count + parts.len() < MAX_PIECES {
                parts.push((m, b, depth + 1));
                parts.push((a, m, depth + 1));
                continue;
            }
            count += 1;
            // (f64::max would drop a NaN)
            worst = if d.is_nan() {
                f64::INFINITY
            } else {
                worst.max(d)
            };
        }
        Ok(worst)
    };
    let vertices = {
        let (v0, v1) = solidcore::topology::edge::start_end_vertices(b, edge).map_err(e)?;
        let mut out = Vec::new();
        for v in [v0, v1].into_iter().flatten() {
            let r = b.vertex(v).map_err(e)?;
            out.push((
                V3 {
                    x: Ivl::from(r.point.x),
                    y: Ivl::from(r.point.y),
                    z: Ivl::from(r.point.z),
                },
                r.tolerance,
            ));
        }
        out
    };
    // the edge between an end `end` of it and the foot of the trim's matching end (at
    // `t`): within the edge's tolerance of the trim's end point P (the bound returned), or
    // else, with P, inside the tolerance ball of a vertex of the edge (the most of their
    // distances to it over its tolerance returned second; the bound then 0: the vertex
    // joins them)
    let stretch = |t: f64, foot: f64, end: f64| -> Result<(f64, f64), String> {
        let (uv, _) = pieces.trim_over(Ivl::from(t))?;
        let p = pieces.surface_over(uv)?[0];
        let (lo, hi) = (foot.min(end), foot.max(end));
        let d = farthest(p, lo, hi, erec.tolerance)?;
        // (a NaN is not within)
        if d <= erec.tolerance {
            return Ok((d, 0.0));
        }
        for &(v, vt) in &vertices {
            let to_v = farthest(v, lo, hi, vt)?.max(norm_hi(sub(p, v)));
            if to_v <= vt {
                return Ok((0.0, to_v / vt));
            }
        }
        Ok((if d.is_nan() { f64::INFINITY } else { d }, 0.0))
    };
    let (ends, at_vertex) = {
        let (x, xv) = stretch(t0, foot0, e0)?;
        let (y, yv) = stretch(t1, foot1, e1)?;
        (x.max(y), xv.max(yv))
    };
    // a stack of pieces (a, b, sa, sb, depth), last first so they are visited in order
    let mut stack: Vec<(f64, f64, f64, f64, u32)> = Vec::new();
    for i in (0..nodes.len() - 1).rev() {
        stack.push((nodes[i], nodes[i + 1], s_nodes[i], s_nodes[i + 1], 0));
    }
    let mut bound = 0.0_f64;
    let mut count = 0usize;
    let mut gap = 0.0_f64;
    let mut last_end: Option<Ivl> = None;
    let mut c1_all = 0.0_f64;
    let mut sampled = 0.0_f64;
    let tolerance = erec.tolerance;
    while let Some((a, bb, sa, sb, depth)) = stack.pop() {
        let (d, d_m, sigma) = pieces.piece(a, bb, sa, sb)?;
        // halving shrinks the slope term only: a bound already near |D(m)| stays
        // (one above the tolerance itself is halved on while that can still decide it)
        let open = d > 1.25 * d_m || (d > tolerance && d_m <= tolerance);
        if d > target && open && depth < MAX_DEPTH && count + stack.len() < MAX_PIECES {
            let m = 0.5 * (a + bb);
            let sm = pieces.foot(pieces.image(m)?, 0.5 * (sa + sb));
            stack.push((m, bb, sm, sb, depth + 1));
            stack.push((a, m, sa, sm, depth + 1));
            continue;
        }
        count += 1;
        bound = bound.max(d);
        sampled = sampled.max(d_m);
        // sigma's ends, enclosed: the start of this piece against the end of the last
        let alpha = (Ivl::from(sb) - Ivl::from(sa)) / (Ivl::from(bb) - Ivl::from(a));
        let at_a = Ivl::from(sa);
        let at_b = Ivl::from(sa) + alpha * (Ivl::from(bb) - Ivl::from(a));
        if let Some(prev) = last_end {
            gap = gap.max(round_up(prev.hull(at_a).hi - prev.hull(at_a).lo));
        }
        last_end = Some(at_b);
        let (_, c1) =
            pieces.curve_over(Ivl::new(sigma.lo.max(pieces.s0), sigma.hi.min(pieces.s1)))?;
        c1_all = c1_all.max(norm_hi(c1));
    }
    // the last piece ends at the last foot: its sigma end enclosed against it
    if let Some(end) = last_end {
        gap = gap.max(round_up(
            end.hull(Ivl::from(foot1)).hi - end.hull(Ivl::from(foot1)).lo,
        ));
    }
    // the pieces cover the edge from foot0 to foot1 (up to the gaps), the stretches the
    // rest: the larger of the two bounds holds everywhere
    let bound = round_up(bound + round_up(c1_all * gap)).max(ends);
    Ok(TrimCertificate {
        edgeuse: eu,
        edge,
        tolerance: erec.tolerance,
        bound,
        pieces: count,
        reversed,
        past: pieces.past.get(),
        sampled,
        at_vertex,
    })
}

/// Enclosures of the images `S(T(t0))`, `S(T(t1))` of the ends of the trim in `eu`'s own
/// slot on its face's surface.
///
/// # Errors
///
/// A missing trim, face or surface; data with no NURBS form.
pub(crate) fn trim_end_images(k: &Kernel, b: &Brep, eu: EdgeuseId) -> Result<[V3<Ivl>; 2], String> {
    let e = |x: solidcore::SolidError| x.to_string();
    let rec = b.edgeuse(eu).map_err(e)?;
    let trim = rec
        .uv_trim_curve
        .as_ref()
        .ok_or("no trim in the edge use's slot")?;
    let face = eu1::face(b, eu).map_err(e)?.ok_or("no face")?;
    let surface = b
        .face(face)
        .map_err(e)?
        .surface
        .as_ref()
        .ok_or("no surface")?;
    let form = as_b_spline(k, crate::certify::measured(surface))
        .map_err(e)?
        .ok_or("no NURBS form")?;
    let sn = Nurbs::of(&form).map_err(e)?;
    let tn = curve_nurbs(trim)?;
    let (t0, t1) = range(&tn);
    let past = Cell::new(0.0);
    let at = |t: f64| -> Result<V3<Ivl>, String> {
        let d = curve_derivs_over(&tn, Ivl::from(t), 0, None).map_err(|x| x.to_string())?;
        let uv = *d.first().ok_or("no enclosure")?;
        Ok(surface_over(&sn, uv, &past)?[0])
    };
    Ok([at(t0)?, at(t1)?])
}

/// Certifies every trim held in an edge use's own slot ([`certify_edgeuse`]), each refined
/// to an eighth of its edge's tolerance.
///
/// # Errors
///
/// The first edge use that cannot be certified, named.
pub(crate) fn certify_brep(k: &Kernel, b: &Brep) -> Result<Vec<TrimCertificate>, String> {
    let mut out = Vec::new();
    for (eu, r) in b.edgeuses().iter() {
        if r.uv_trim_curve.is_none() {
            continue;
        }
        let tol = match r.edge {
            Some(e) => b.edge(e).map_err(|x| x.to_string())?.tolerance,
            None => return Err(format!("edge use {}: no edge", eu.index())),
        };
        out.push(
            certify_edgeuse(k, b, eu, 0.125 * tol)
                .map_err(|x| format!("edge use {}: {x}", eu.index()))?,
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part ending on an interior knot takes the span on its own side, even where its
    /// middle rounds onto the knot (the float just below 1.0 to 1.0); a point part on the
    /// knot is listed on both spans that meet there.
    #[test]
    fn cuts_pick_each_parts_span_from_its_ends() {
        let u = [0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0];
        let below = 1.0_f64.next_down();
        assert_eq!(0.5 * (below + 1.0), 1.0, "the middle rounds onto the knot");
        assert_eq!(cuts(&u, 2, 4, below, 1.0), vec![(below, 1.0, 2)]);
        assert_eq!(cuts(&u, 2, 4, 1.0, 1.0), vec![(1.0, 1.0, 3), (1.0, 1.0, 2)]);
        assert_eq!(cuts(&u, 2, 4, 0.5, 1.5), vec![(0.5, 1.0, 2), (1.0, 1.5, 3)]);
    }

    /// A box's trims (made by the kernel) are certified on their edges far inside the
    /// tolerance; a trim put on another edge's use of the same face is bounded at least
    /// as far as the edges are apart.
    #[test]
    fn a_box_trims_lie_on_their_edges_and_a_moved_one_does_not() {
        let mut k = Kernel::default();
        let mut b = solidcore::primitive::api_primitives::create_box(
            &mut k,
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let ids: Vec<EdgeuseId> = b.edgeuses().iter().map(|(eu, _)| eu).collect();
        for &eu in &ids {
            let f = solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                &k, &mut b, eu, false, false, true,
            )
            .unwrap();
            f.status.unwrap();
        }
        let certs = certify_brep(&k, &b).unwrap();
        // one trim per edge and face (12 x 2): a face's two sides share theirs (the mate's slot)
        assert_eq!(certs.len(), 24);
        for c in &certs {
            eprintln!("{c:?}");
            assert!(c.holds(), "{c:?}");
            assert!(c.bound < 1e-10, "{c:?}");
        }
        // the trim of the next edge use around the loop, on this one
        let eu = ids[0];
        let next = b.edgeuse(eu).unwrap().ccw.unwrap();
        let moved = b.edgeuse(next).unwrap().uv_trim_curve.clone();
        b.edgeuse_mut(eu).unwrap().uv_trim_curve = moved;
        match certify_edgeuse(&k, &b, eu, 1.0) {
            Ok(c) => assert!(c.bound >= 5.0, "{c:?}"),
            Err(e) => assert!(e.contains("sigma"), "{e}"),
        }
    }

    /// An edge whose curve is a curve on a surface (`CrvOnSurf`): with the face's own trim
    /// as its UV curve on the face's surface, the trim is certified on it; with the next
    /// edge use's trim as its UV curve (the curve now runs along another edge of the
    /// face), it is not.
    #[test]
    fn a_trim_on_a_curve_on_surface_edge() {
        use solidcore::curve::crv_on_surf::CrvOnSurf;
        use solidcore::geom::extent1d::Extent1d;
        let mut k = Kernel::default();
        let mut b = solidcore::primitive::api_primitives::create_box(
            &mut k,
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let ids: Vec<EdgeuseId> = b.edgeuses().iter().map(|(eu, _)| eu).collect();
        for &eu in &ids {
            solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                &k, &mut b, eu, false, false, true,
            )
            .unwrap()
            .status
            .unwrap();
        }
        // an edge use holding its trim in its own slot (a face's two sides share one)
        let eu = *ids
            .iter()
            .find(|&&x| b.edgeuse(x).unwrap().uv_trim_curve.is_some())
            .unwrap();
        let next = b.edgeuse(eu).unwrap().ccw.unwrap();
        let face = eu1::face(&b, eu).unwrap().unwrap();
        let surface = b.face(face).unwrap().surface.clone().unwrap();
        let edge = b.edgeuse(eu).unwrap().edge.unwrap();
        let on = |b: &mut Brep, k: &Kernel, uv: Curve| {
            let range = curve_nurbs(&uv).map(|n| range(&n)).unwrap();
            let c = CrvOnSurf::new(k, uv, surface.clone(), None, 0).unwrap();
            let rec = b.edge_mut(edge).unwrap();
            rec.curve = Some(Curve::from(c));
            rec.interval = Extent1d::from_min_max(range.0, range.1);
        };
        let own = b.edgeuse(eu).unwrap().uv_trim_curve.clone().unwrap();
        on(&mut b, &k, own);
        let c = certify_edgeuse(&k, &b, eu, 1e-12).unwrap();
        assert!(c.holds() && c.bound < 1e-10, "{c:?}");
        // the next edge use's trim, from its slot or its mate's (the same face)
        let other = eu1::uv_trim_curve(&b, next).unwrap().0.unwrap().clone();
        on(&mut b, &k, other);
        match certify_edgeuse(&k, &b, eu, 1.0) {
            Ok(c) => assert!(c.bound >= 5.0, "{c:?}"),
            Err(e) => assert!(e.contains("sigma"), "{e}"),
        }
    }

    /// An edge running `delta` past its trim's end along its own curve (the trim's end
    /// short of the edge's end, as a fillet's trims can be): the edge's end point is
    /// `delta` from the trim's image, and the bound pays that once, so a tolerance of
    /// `1.5 delta` holds (an end piece pinned to the edge's end paid about `2 delta`). The
    /// true distance is `delta`, so a tolerance below it is still refused.
    #[test]
    fn a_trim_ending_short_of_its_edge_end_pays_the_offset_once() {
        use solidcore::geom::extent1d::Extent1d;
        let mut k = Kernel::default();
        let mut b = solidcore::primitive::api_primitives::create_box(
            &mut k,
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let ids: Vec<EdgeuseId> = b.edgeuses().iter().map(|(eu, _)| eu).collect();
        for &eu in &ids {
            solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                &k, &mut b, eu, false, false, true,
            )
            .unwrap()
            .status
            .unwrap();
        }
        let eu = *ids
            .iter()
            .find(|&&x| b.edgeuse(x).unwrap().uv_trim_curve.is_some())
            .unwrap();
        let edge = b.edgeuse(eu).unwrap().edge.unwrap();
        let delta = 1e-3;
        let speed = {
            let rec = b.edge(edge).unwrap();
            let (s0, s1) = (rec.interval.min(), rec.interval.max());
            let n = curve_nurbs(rec.curve.as_ref().unwrap()).unwrap();
            let d = curve_derivs_over(&n, Ivl::from(0.5 * (s0 + s1)), 1, None).unwrap();
            norm_hi(d[1])
        };
        {
            let rec = b.edge_mut(edge).unwrap();
            let (s0, s1) = (rec.interval.min(), rec.interval.max());
            rec.interval = Extent1d::from_min_max(s0, s1 + delta / speed);
        }
        let with = |b: &mut Brep, tol: f64| {
            b.edge_mut(edge).unwrap().tolerance = tol;
            certify_edgeuse(&k, b, eu, 0.125 * tol).unwrap()
        };
        let c = with(&mut b, 1.5 * delta);
        assert!(c.holds(), "{c:?}");
        // (a part stops halving once its bound is within a quarter of its midpoint's)
        assert!(
            c.bound >= delta * (1.0 - 1e-9) && c.bound <= 1.25 * delta * (1.0 + 1e-9),
            "{c:?}"
        );
        let c = with(&mut b, 0.9 * delta);
        assert!(!c.holds() && c.at_vertex == 0.0, "{c:?}");
        // the edge's vertices' balls widened to 1.2 delta: the stretch past the trim's end
        // (delta long, from the box corner where the trim ends) lies in the corner's ball
        let (v0, v1) = solidcore::topology::edge::start_end_vertices(&b, edge).unwrap();
        for v in [v0, v1].into_iter().flatten() {
            b.vertex_mut(v).unwrap().tolerance = 1.2 * delta;
        }
        let c = with(&mut b, 0.9 * delta);
        assert!(
            c.holds() && c.at_vertex > 0.8 && c.at_vertex <= 1.0,
            "{c:?}"
        );
        // a ball too small for the stretch still refuses
        for v in [v0, v1].into_iter().flatten() {
            b.vertex_mut(v).unwrap().tolerance = 0.95 * delta;
        }
        let c = with(&mut b, 0.9 * delta);
        assert!(!c.holds() && c.at_vertex == 0.0, "{c:?}");
    }
}
