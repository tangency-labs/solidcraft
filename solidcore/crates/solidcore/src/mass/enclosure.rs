//! A certified enclosure `[lo, hi]` of a closed body's volume, and the volume's sign
//! decided from it (not upstream).
//!
//! The volume is the flux of `F = (0, 0, z - z0)` (`div F = 1`) out of the body, face by
//! face, as [`super::integrator::properties`] integrates it: on a face with surface `S` the
//! integral of `g = (S_z - z0) (S_u x S_v)_z` over its trimmed domain is, by Green's
//! theorem, `-oint G du` round its trims, `G(u, v)` the integral of `g` along v from a base
//! `vb`. The estimate integrates that with an adaptive quadrature whose error it estimates;
//! here every part is enclosed instead:
//!
//! - **Cells.** Each trim piece (a polynomial piece of a pcurve, or a straight uv segment
//!   closing a loop between two of them) is swept down to the base piecewise between the
//!   surface's v knots, `phi(t, sigma) = (u(t), a + sigma b(t))`, `sigma` in `[0, 1]`: a
//!   whole v span (`a` a knot, `b` its length) or the part up to the trim
//!   (`b(t) = v(t) - a`). On a cell of `(t, sigma)` the integrand `-(S_z - z0) (S_u x S_v)_z u'(t) b(t)` at
//!   `phi(t, sigma)` (`u' b` the Jacobian of `phi`; in this factored form no product cancels
//!   another, which an interval could not follow) is a Taylor model [1, 2]: its Taylor
//!   polynomial of degree `ORDER - 1` at the cell's middle, every coefficient enclosed at
//!   that point and the polynomial integrated exactly, and the Lagrange remainder from the
//!   coefficients of degree `ORDER` enclosed over the whole cell (rounded interval
//!   arithmetic, [`Ivl`]; the surface's partials over a box, [`nurbs_partials_within_span`]).
//!   A cell is first evaluated at the low degree [`FIRST_ORDER`] and at `ORDER` only once it
//!   is the widest. The expansion holds where the cell's image lies on one polynomial piece
//!   of the surface; a cell whose image may cross a knot line is enclosed by the integrand's
//!   range over it times its area [3, ch. 9], which holds whatever the continuity there.
//! - **Gaps.** Two faces' trimmed boundaries along an edge need not meet: their curves
//!   `X_1(t)`, `X_2(t)` (one edge parameter) are joined by the ruled strip
//!   `X_1 + s (X_2 - X_1)`, whose flux is at most `max |z - z0|` times its area, at most
//!   `int |D| (|X_1'| + |D'|) dt` with `D = X_2 - X_1`, enclosed by the same Taylor models.
//!   An edge's tolerance is not used: it is measured from samples
//!   (`docs/validity-and-tolerances.md` §5.2), not a bound. What is left open at the
//!   vertices (the strips' ends, the loops' closing segments) is a set of closed curves of
//!   total length `L` at most, capped by cones of area at most `L^2 / 4`.
//!
//! The faces, strips and caps are a closed surface, and its flux is the volume it bounds,
//! each point counted by its winding number: for a closed body whose faces meet, the body's
//! volume. A face is integrated on its surface as stored: a definition's net, which a
//! certified face holds within its bound of the definition (that bound is not added here).
//! The enclosure refines the cell of widest enclosure, one bisection at a time,
//! until it is as narrow as asked ([`enclose_volume`]) or decides the sign
//! ([`volume_sign`]), or until [`CELL_BUDGET`] cells are spent.
//!
//! References: [1] G. F. Corliss, L. B. Rall, Adaptive, self-validating numerical
//! quadrature, *SIAM J. Sci. Stat. Comput.* 8(5) (1987); [2] K. Makino, M. Berz, Taylor
//! models and other validated functional inclusion methods, *Int. J. Pure Appl. Math.* 4
//! (2003); [3] R. E. Moore, R. B. Kearfott, M. J. Cloud, *Introduction to Interval
//! Analysis*, SIAM (2009).
//!
//! Deviations: none (not upstream). Bounded work: at most [`CELL_BUDGET`] cells and
//! [`MAX_DEPTH`] bisections of one cell, no recursion (a work list ordered by width, then by
//! insertion), the kernel's cancel token polled every 64 cells.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_within_span, partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, knot, span, Nurbs, Partials};
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use crate::{Result, SolidError};

const OP: &str = "mass::enclosure";

/// The degree of a cell's remainder coefficients: its Taylor polynomial has degree
/// `ORDER - 1`, and the remainder shrinks with the cell's size to the power `ORDER + 1`.
pub const ORDER: usize = 8;

/// The remainder degree a cell is first evaluated at: low, so a body that decides at once
/// (most do) costs little; a cell is evaluated again at [`ORDER`] when it is the widest,
/// before it is cut.
pub const FIRST_ORDER: usize = 3;

/// The cells past which an enclosure is no longer refined (the starting cells, one or a
/// few per trim piece and edge, and the last bisection's two come on top). A bound on the
/// work of one decision, not fitted to a case: the rig's slowest decision takes about 10^5.
pub const CELL_BUDGET: usize = 1 << 17;

/// The most bisections from a starting cell to one of its cells: past it a cell is kept
/// as it is (its parameters are then within a few roundings of each other).
pub const MAX_DEPTH: u32 = 60;

/// A cell's series are needed one order past `ORDER`: the integrand is built from `X_t` and
/// `X_sigma`, each one order below `X`.
const XORD: usize = ORDER + 1;
/// The coefficients of a series in two variables of total degree at most `XORD`.
const LEN: usize = (XORD + 1) * (XORD + 2) / 2;

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// What an enclosure cost, machine independent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnclosureCounts {
    /// Cells evaluated, strips included.
    pub cells: usize,
    /// Cells enclosed by their Taylor model.
    pub taylor: usize,
    /// Cells enclosed by their integrand's range (their image may cross a knot line).
    pub ranged: usize,
    /// Strip cells (the gaps between faces).
    pub strips: usize,
}

/// A certified enclosure of a body's volume: the volume is in `[lo, hi]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeEnclosure {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
    /// What it cost.
    pub counts: EnclosureCounts,
}

impl VolumeEnclosure {
    /// `hi - lo`.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.hi - self.lo
    }
}

/// The sign of a body's volume, decided from its enclosure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VolumeSign {
    /// `lo > 0`: the volume is positive.
    Positive(VolumeEnclosure),
    /// `hi < 0`: the volume is negative (the body is inside out).
    Negative(VolumeEnclosure),
    /// The enclosure still holds 0 when refinement ends: the cells that no cut narrows (gaps
    /// between faces) already hold it whatever the rest becomes, [`CELL_BUDGET`] cells are
    /// spent, or every cell is cut [`MAX_DEPTH`] times.
    Undecided(VolumeEnclosure),
}

/// An enclosure of the volume of the closed body `b` at most `width` wide, or the
/// narrowest [`CELL_BUDGET`] cells give. Missing UV trim curves are made on `b`, as
/// [`super::integrator::properties::brep_volume_default`] makes them.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `width` that is not positive, a face whose sides are
/// not one material region and one void, an edge whose face sides do not pair in opposite
/// senses, two pcurves of an edge that share no parameters, or a loop's end that is not
/// finite; [`SolidError::Unsupported`] for a surface or pcurve with no NURBS form, a face
/// without trim loops, weights not proved positive at a point or along a trim, or a
/// parameter box that is not finite; [`SolidError::UnsureRegion`];
/// [`SolidError::Cancelled`]; those of the evaluations.
pub fn enclose_volume(k: &Kernel, b: &mut Brep, width: f64) -> Result<VolumeEnclosure> {
    if width.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "width not positive"));
    }
    let mut w = Work::new(k, b)?;
    // past a width the settled cells alone exceed, no refining narrows it
    w.refine(|e| e.hi - e.lo <= width, |_, still| still + still > width)
}

/// The sign of the volume of the closed body `b`, from its enclosure refined until it
/// decides; [`VolumeSign::Undecided`] only when the enclosure still holds 0 when
/// refinement ends ([`VolumeSign::Undecided`] says when).
///
/// # Errors
///
/// Those of [`enclose_volume`].
pub fn volume_sign(k: &Kernel, b: &mut Brep) -> Result<VolumeSign> {
    volume_sign_within(k, b, CELL_BUDGET)
}

/// [`volume_sign`] within `budget` cells.
fn volume_sign_within(k: &Kernel, b: &mut Brep, budget: usize) -> Result<VolumeSign> {
    let mut w = Work::new(k, b)?;
    w.budget = budget;
    // the settled cells and the fixed terms are symmetric about 0: once their half width is
    // at least the magnitude of everything else, 0 stays inside however the rest is refined
    let e = w.refine(
        |e| e.lo > 0.0 || e.hi < 0.0,
        |rest, still| still >= rest.mag(),
    )?;
    Ok(if e.lo > 0.0 {
        VolumeSign::Positive(e)
    } else if e.hi < 0.0 {
        VolumeSign::Negative(e)
    } else {
        VolumeSign::Undecided(e)
    })
}

// ---- series in two variables ----------------------------------------------------------

/// The index of `x^i y^j` in a [`Bs`].
const fn ix(i: usize, j: usize) -> usize {
    let d = i + j;
    d * (d + 1) / 2 + j
}

/// A truncated power series in `x` (along `t`) and `y` (along `sigma`) with enclosed
/// coefficients, of total degree at most `XORD`.
#[derive(Clone, Copy, Debug)]
struct Bs {
    c: [Ivl; LEN],
}

impl Bs {
    const fn zero() -> Self {
        Self { c: [IZ; LEN] }
    }

    fn constant(a: Ivl) -> Self {
        let mut s = Self::zero();
        s.set(0, 0, a);
        s
    }

    fn at(&self, i: usize, j: usize) -> Ivl {
        if i + j > XORD {
            return IZ;
        }
        self.c.get(ix(i, j)).copied().unwrap_or(IZ)
    }

    fn set(&mut self, i: usize, j: usize, v: Ivl) {
        if i + j > XORD {
            return;
        }
        if let Some(c) = self.c.get_mut(ix(i, j)) {
            *c = v;
        }
    }

    fn add(&self, o: &Self) -> Self {
        let mut r = *self;
        for (a, b) in r.c.iter_mut().zip(&o.c) {
            if !is_zero(*b) {
                *a = *a + *b;
            }
        }
        r
    }

    fn sub(&self, o: &Self) -> Self {
        let mut r = *self;
        for (a, b) in r.c.iter_mut().zip(&o.c) {
            if !is_zero(*b) {
                *a = *a - *b;
            }
        }
        r
    }

    fn scale(&self, s: Ivl) -> Self {
        let mut r = *self;
        for a in &mut r.c {
            if !is_zero(*a) {
                *a = *a * s;
            }
        }
        r
    }

    /// The product, truncated at total degree `n`.
    fn mul(&self, o: &Self, n: usize) -> Self {
        let mut r = Self::zero();
        for d1 in 0..=n {
            for j1 in 0..=d1 {
                let a = self.at(d1 - j1, j1);
                if is_zero(a) {
                    continue;
                }
                for d2 in 0..=(n - d1) {
                    for j2 in 0..=d2 {
                        let b = o.at(d2 - j2, j2);
                        if is_zero(b) {
                            continue;
                        }
                        let (i, j) = (d1 - j1 + d2 - j2, j1 + j2);
                        r.set(i, j, r.at(i, j) + a * b);
                    }
                }
            }
        }
        r
    }

    /// `d/dx`, of total degree `n - 1` for a series of degree `n`.
    fn dx(&self, n: usize) -> Self {
        let mut r = Self::zero();
        for d in 1..=n {
            for j in 0..d {
                let i = d - j;
                let a = self.at(i, j);
                if !is_zero(a) {
                    r.set(i - 1, j, a * Ivl::from(i as f64));
                }
            }
        }
        r
    }
}

/// The point interval 0 (an exact zero coefficient, skipped by the products).
fn is_zero(a: Ivl) -> bool {
    a.lo == 0.0 && a.hi == 0.0
}

/// `1 / n!` enclosed.
fn inverse_factorial(n: usize) -> Ivl {
    let mut f = 1.0_f64;
    for i in 2..=n {
        f *= i as f64; // exact up to 18!
    }
    Ivl::from(1.0) / Ivl::from(f)
}

/// `X = sum P_ab / (a! b!) du^a dv^b`, `a + b <= n`: the surface composed with the cell's
/// parameters, `du`, `dv` their series without constant term and `P` the surface's
/// partials at the expansion point (or, for coefficients over a cell, over the box its
/// points map into).
fn compose(p: &Partials<Ivl>, du: &Bs, dv: &Bs, n: usize) -> [Bs; 3] {
    compose_partial(p, (0, 0), du, dv, n)
}

/// [`compose`] of the surface's partial `(da, db)` (`S_u` for `(1, 0)`): its Taylor
/// coefficients are the surface's partials shifted by that order.
fn compose_partial(
    p: &Partials<Ivl>,
    (da, db): (usize, usize),
    du: &Bs,
    dv: &Bs,
    n: usize,
) -> [Bs; 3] {
    let mut dv_pow = vec![Bs::constant(Ivl::from(1.0))];
    for b in 1..=n {
        let next = dv_pow.get(b - 1).map_or(Bs::zero(), |q| q.mul(dv, n));
        dv_pow.push(next);
    }
    let mut x = [Bs::zero(); 3];
    let mut du_pow = Bs::constant(Ivl::from(1.0));
    for a in 0..=n {
        let mut inner = [Bs::zero(); 3];
        for (b, q) in dv_pow.iter().enumerate().take(n - a + 1) {
            let f = inverse_factorial(a) * inverse_factorial(b);
            let pab = p.at(a + da, b + db);
            for (acc, c) in inner.iter_mut().zip([pab.x, pab.y, pab.z]) {
                if !is_zero(c) {
                    *acc = acc.add(&q.scale(c * f));
                }
            }
        }
        for (xc, ic) in x.iter_mut().zip(&inner) {
            *xc = xc.add(&du_pow.mul(ic, n));
        }
        du_pow = du_pow.mul(du, n);
    }
    x
}

// ---- the body's parts ------------------------------------------------------------------

/// A face's surface and the levels its trims are swept down to.
struct FaceGeo {
    cn: Nurbs,
    /// The base `vb` (at most every trim's lowest v), then the surface's v knots above it
    /// and below the trims' highest v, ascending.
    levels: Vec<f64>,
    /// -1 where the face's material side is its upward face use's (`Same`), as the
    /// integrator signs it ([`super::integrator::properties::boundary_properties`]).
    sign: f64,
}

/// A pcurve piece of one face's loop, integrated from `t.0` to `t.1` along its parameter
/// and counted `dir` times (-1: the edge use runs against its curve).
struct Piece {
    face: usize,
    cn: NurbsCurve,
    span: usize,
    dir: f64,
}

/// An edge's two boundary curves on its faces, over their common parameters.
struct StripGeo {
    sides: [(usize, NurbsCurve); 2],
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    /// A sweep of piece `piece` over the v span from `levels[j]` (the whole span when
    /// `whole`, else up to the trim).
    Sweep { piece: usize, j: usize, whole: bool },
    /// The gap strip of strip `strip`, on the polynomial pieces `spans` of its curves.
    Strip { strip: usize, spans: [usize; 2] },
}

#[derive(Clone, Copy, Debug)]
struct Item {
    kind: Kind,
    t: (f64, f64),
    s: (f64, f64),
    depth: u32,
    /// The remainder degree it is evaluated at ([`FIRST_ORDER`], then [`ORDER`]).
    order: usize,
    /// A strip cell whose width is mostly a gap no cut removes ([`Work::strip_cell`]): kept
    /// as it is.
    settled: bool,
    enc: Ivl,
    split_t: bool,
}

/// The enclosure under construction.
struct Work<'a> {
    k: &'a Kernel,
    z0: f64,
    faces: Vec<FaceGeo>,
    pieces: Vec<Piece>,
    strips: Vec<StripGeo>,
    items: Vec<Item>,
    alive: Vec<bool>,
    heap: BinaryHeap<(u64, Reverse<usize>)>,
    /// The caps' and the loops' rounding terms, `[-w, w]`.
    fixed: Ivl,
    /// The cells past which it is no longer refined ([`CELL_BUDGET`]).
    budget: usize,
    counts: EnclosureCounts,
}

/// The polynomial pieces of a curve: `(span, start, end)` for every non-empty knot span.
fn curve_spans(cn: &NurbsCurve) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    for k in cn.p..cn.n {
        let (a, b) = (knot(&cn.u, k as isize), knot(&cn.u, k as isize + 1));
        if a < b {
            out.push((k, a, b));
        }
    }
    out
}

/// The control points' range in u and v of the curve's polynomial piece `span` (where the
/// piece lies: its weights are positive).
fn piece_hull(cn: &NurbsCurve, span: usize) -> (Ivl, Ivl) {
    let mut u = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    let mut v = u;
    for i in (span + 1).saturating_sub(cn.p + 1)..=span {
        let at = |c: usize| cn.pw.get(4 * i + c).copied().unwrap_or(f64::NAN);
        let w = at(3);
        // a point x w / w, widened by its rounding where the weight is not 1
        let point = |x: f64| {
            if w == 1.0 {
                Ivl::from(x)
            } else {
                Ivl::from(x) / Ivl::from(w)
            }
        };
        let (p, q) = (point(at(0)), point(at(1)));
        u = u.hull(p);
        v = v.hull(q);
    }
    (u, v)
}

/// The closed parameter range of span `k` of `knots` (degree `p`, `n` control points), the
/// end spans continued without end.
fn span_range(knots: &[f64], p: usize, n: usize, k: usize) -> (f64, f64) {
    let lo = if k <= p {
        f64::NEG_INFINITY
    } else {
        knot(knots, k as isize)
    };
    let hi = if k + 1 >= n {
        f64::INFINITY
    } else {
        knot(knots, k as isize + 1)
    };
    (lo, hi)
}

/// The surface's piece `(ku, kv)` whose closed span holds the box, if one does.
fn piece_of(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<(usize, usize)> {
    let ku = span(&cn.u, cn.p, cn.nu, u.mid());
    let kv = span(&cn.v, cn.q, cn.nv, v.mid());
    let (ua, ub) = span_range(&cn.u, cn.p, cn.nu, ku);
    let (va, vb) = span_range(&cn.v, cn.q, cn.nv, kv);
    let inside = u.lo >= ua && u.hi <= ub && v.lo >= va && v.hi <= vb;
    inside.then_some((ku, kv))
}

/// The partials of piece `(ku, kv)` over the box ([`nurbs_partials_within_span`]), or `None`
/// where its weights are not proved positive over it (its one failure: a box too large for
/// the interval evaluation, which a smaller one proves; a cell answers it by refining).
fn piece_partials(
    cn: &Nurbs,
    (ku, kv): (usize, usize),
    u: Ivl,
    v: Ivl,
    lk: usize,
) -> Option<Partials<Ivl>> {
    nurbs_partials_within_span(cn, ku, kv, u.lo, u.hi, v.lo, v.hi, lk).ok()
}

/// The surface's partials up to order `lk` at every point of the box, from every piece it
/// meets, hulled; `None` where a piece's weights are not proved positive over it
/// ([`piece_partials`]).
fn partials_over(cn: &Nurbs, u: Ivl, v: Ivl, lk: usize) -> Result<Option<Partials<Ivl>>> {
    let bad = || SolidError::Unsupported {
        operation: OP,
        reason: "a parameter box that is not finite",
    };
    if !(u.lo.is_finite() && u.hi.is_finite() && v.lo.is_finite() && v.hi.is_finite()) {
        return Err(bad());
    }
    let (ku0, ku1) = (
        span(&cn.u, cn.p, cn.nu, u.lo),
        span(&cn.u, cn.p, cn.nu, u.hi),
    );
    let (kv0, kv1) = (
        span(&cn.v, cn.q, cn.nv, v.lo),
        span(&cn.v, cn.q, cn.nv, v.hi),
    );
    let mut out: Option<Partials<Ivl>> = None;
    for ku in ku0..=ku1 {
        let (ua, ub) = span_range(&cn.u, cn.p, cn.nu, ku);
        let (a0, a1) = (u.lo.max(ua), u.hi.min(ub));
        if a0
            .partial_cmp(&a1)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
            || (ku > cn.p
                && ku + 1 < cn.nu
                && ua.partial_cmp(&ub) != Some(core::cmp::Ordering::Less))
        {
            continue;
        }
        for kv in kv0..=kv1 {
            let (va, vb) = span_range(&cn.v, cn.q, cn.nv, kv);
            let (b0, b1) = (v.lo.max(va), v.hi.min(vb));
            if b0
                .partial_cmp(&b1)
                .is_none_or(|o| o == core::cmp::Ordering::Greater)
                || (kv > cn.q
                    && kv + 1 < cn.nv
                    && va.partial_cmp(&vb) != Some(core::cmp::Ordering::Less))
            {
                continue;
            }
            let Some(p) = piece_partials(cn, (ku, kv), Ivl::new(a0, a1), Ivl::new(b0, b1), lk)
            else {
                return Ok(None);
            };
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.map(Some).ok_or_else(bad)
}

/// [`partials_over`] where a point's box must be proved: the box is a point's enclosure,
/// within a few roundings of it.
fn partials_at(cn: &Nurbs, u: Ivl, v: Ivl, lk: usize) -> Result<Partials<Ivl>> {
    partials_over(cn, u, v, lk)?.ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a surface whose weights are not proved positive at a point",
    })
}

/// A bound on `int |g(u, s)| ds` from `a` to `b` for every `u` in `u`, `g = (S_z - z0)
/// (S_u x S_v)_z`: the interval cut in halves (at most 2^16 parts) until each part's
/// partials are proved.
fn g_integral(cn: &Nurbs, u: Ivl, a: f64, b: f64, z0: f64) -> Result<f64> {
    let (a, b) = (a.min(b), a.max(b));
    let mut sum = Ivl::from(0.0);
    let mut stack = vec![(a, b, 0u32)];
    while let Some((s0, s1, depth)) = stack.pop() {
        if let Some(p) = partials_over(cn, u, Ivl::new(s0, s1), 1)? {
            let (s, su, sv) = (p.at(0, 0), p.at(1, 0), p.at(0, 1));
            let g = (s.z - Ivl::from(z0)) * (su.x * sv.y - su.y * sv.x);
            sum = sum + Ivl::from(g.mag()) * (Ivl::from(s1) - Ivl::from(s0));
            continue;
        }
        let m = 0.5 * (s0 + s1);
        if depth >= 16 || !(s0 < m && m < s1) {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a surface whose weights are not proved positive along a trim",
            });
        }
        stack.push((m, s1, depth + 1));
        stack.push((s0, m, depth + 1));
    }
    Ok(sum.hi)
}

/// The largest magnitude of a vector enclosure's length, bounded by its components' sum.
fn mag3(v: [Ivl; 3]) -> f64 {
    v.iter()
        .fold(Ivl::from(0.0), |sum, c| sum + Ivl::from(c.mag()))
        .hi
}

/// `hi - lo` of an interval, rounded up: a bound on its width.
fn width(x: Ivl) -> f64 {
    (Ivl::from(x.hi) - Ivl::from(x.lo)).hi
}

/// `int |x|^i` over `[xl, xh]` (`xl <= 0 <= xh`), enclosed: also `int x^i` for even `i`.
fn abs_moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    let nl = Ivl::from(0.0) - xl;
    for _ in 0..=i {
        a = a * xh;
        b = b * nl;
    }
    (a + b) / Ivl::from((i + 1) as f64)
}

/// `int x^i` over `[xl, xh]`, enclosed.
fn moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    for _ in 0..=i {
        a = a * xh;
        b = b * xl;
    }
    (a - b) / Ivl::from((i + 1) as f64)
}

/// The series of a curve's parameter `c` (0: u, 1: v) about a point (`over` false, the
/// coefficients at `t`) or every point of `t` (`over` true), from its derivatives, without
/// its constant term (returned apart).
fn curve_series(d: &[V3<Ivl>], c: usize, n: usize) -> (Ivl, Bs) {
    let pick = |v: &V3<Ivl>| if c == 0 { v.x } else { v.y };
    let mut s = Bs::zero();
    for kk in 1..=n {
        if let Some(v) = d.get(kk) {
            s.set(kk, 0, pick(v) * inverse_factorial(kk));
        }
    }
    (d.first().map_or(Ivl::whole(), pick), s)
}

impl<'a> Work<'a> {
    fn new(k: &'a Kernel, b: &mut Brep) -> Result<Self> {
        let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
        let z0 = bbox.mid().z;
        let mut w = Self {
            k,
            z0,
            faces: Vec::new(),
            pieces: Vec::new(),
            strips: Vec::new(),
            items: Vec::new(),
            alive: Vec::new(),
            heap: BinaryHeap::new(),
            fixed: IZ,
            budget: CELL_BUDGET,
            counts: EnclosureCounts::default(),
        };
        let mut sides: BTreeMap<EdgeId, Vec<(usize, NurbsCurve, bool)>> = BTreeMap::new();
        // the closing pieces' and rounding gaps' lengths in 3D, and the greatest |z - z0|
        // near them
        let mut open_length = Ivl::from(0.0);
        let mut zcap = 0.0_f64;
        let faces = crate::topology::brep::p1::faces(b, None)?;
        for f in faces {
            k.check_cancelled(OP)?;
            w.add_face(k, b, f, &mut sides, &mut open_length, &mut zcap)?;
        }
        // each edge's face sides in pairs of opposite sense along it, a strip for each pair
        // (two sides at a manifold edge; four where solids touch along it)
        for (_, s) in sides {
            let (forward, backward): (Vec<_>, Vec<_>) = s.into_iter().partition(|x| x.2);
            if forward.len() != backward.len() || forward.is_empty() {
                return Err(SolidError::invalid_input(
                    OP,
                    "an edge whose face sides do not pair in opposite senses",
                ));
            }
            for (a, c) in forward.into_iter().zip(backward) {
                w.add_strip((a.0, a.1), (c.0, c.1), &mut open_length, &mut zcap)?;
            }
        }
        // the caps: closed curves of total length L, each within L of the points near it
        let l = open_length.hi;
        let cap = (Ivl::from(zcap) + Ivl::from(l)) * Ivl::from(l) * Ivl::from(l) / Ivl::from(4.0);
        w.fixed = w.fixed + Ivl::new(-cap.hi, cap.hi);
        Ok(w)
    }

    fn add_face(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        f: FaceId,
        sides: &mut BTreeMap<EdgeId, Vec<(usize, NurbsCurve, bool)>>,
        open_length: &mut Ivl,
        zcap: &mut f64,
    ) -> Result<()> {
        use crate::topology::edgeuse::p1 as eu1;
        let up = crate::topology::face::p2::upward_faceuse(b, f)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let mate = b
            .faceuse(up)?
            .mate
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let material = |fu| -> Result<bool> {
            match eu1::faceuse_region(b, fu)? {
                Some(r) => region_material(b, r),
                None => Ok(false),
            }
        };
        let (mu, mm) = (material(up)?, material(mate)?);
        if mu == mm {
            return Err(SolidError::invalid_input(
                OP,
                "a face whose sides are not one material region and one void",
            ));
        }
        let fu = if mu { up } else { mate };
        let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
            -1.0
        } else {
            1.0
        };
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let cn = match as_b_spline(k, surface)? {
            Some(bs) => Nurbs::of(&bs)?,
            None => {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a surface with no NURBS form",
                })
            }
        };
        // the loops in the order and direction the integrator walks them
        // (`create_oriented_trim_boundary`): the upward use's loops, each edge use's
        // pcurve forward where it is `Same`, backward where `Opposite`
        let mut loops: Vec<Vec<(EdgeId, NurbsCurve, f64)>> = Vec::new();
        for lu in eu1::faceuse_loopuses(b, up, false, None)? {
            if crate::topology::loopuse::is_vertex_loopuse(b, lu)? {
                continue;
            }
            let mut lp = Vec::new();
            for eu in eu1::loopuse_edgeuses(b, lu)? {
                let dir = if eu1::orientation(b, eu)? == OrientType::Same {
                    1.0
                } else {
                    -1.0
                };
                let e = b
                    .edgeuse(eu)?
                    .edge
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                lp.push((e, pcurve(k, b, eu)?, dir));
            }
            loops.push(lp);
        }
        let face = self.faces.len();
        // the levels: the base below every trim, then the v knots between
        let (mut vlo, mut vhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, c, _) in loops.iter().flatten() {
            for (s, _, _) in curve_spans(c) {
                let (_, v) = piece_hull(c, s);
                vlo = vlo.min(v.lo);
                vhi = vhi.max(v.hi);
            }
        }
        if vlo
            .partial_cmp(&vhi)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
        {
            // a face with no trim loops is its whole surface (a closed one), which the
            // sweep down to a base does not cover
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a face without trim loops",
            });
        }
        let mut levels = vec![vlo];
        for &x in &cn.v {
            if x > vlo && x < vhi && levels.last().is_some_and(|&l| x > l) {
                levels.push(x);
            }
        }
        self.faces.push(FaceGeo { cn, levels, sign });
        for lp in loops {
            // each curve's start and end point (as walked), enclosed
            let mut ends = Vec::new();
            for (_, c, dir) in &lp {
                let (t0, t1) = (knot(&c.u, c.p as isize), knot(&c.u, c.n as isize));
                let (a, z) = if *dir > 0.0 { (t0, t1) } else { (t1, t0) };
                let pa = curve_derivs_over(c, Ivl::from(a), 0, None)?;
                let pz = curve_derivs_over(c, Ivl::from(z), 0, None)?;
                let first =
                    |d: &[V3<Ivl>]| d.first().copied().ok_or_else(|| SolidError::sm_err(OP));
                ends.push((first(&pa)?, first(&pz)?));
            }
            let count = lp.len();
            for (i, (e, c, dir)) in lp.into_iter().enumerate() {
                // the segment from the previous curve's end to this one's start
                let before = if i == 0 {
                    count.saturating_sub(1)
                } else {
                    i - 1
                };
                let prev = ends.get(before).map(|x| x.1);
                let this = ends.get(i).map(|x| x.0);
                if let (Some(p), Some(q)) = (prev, this) {
                    let (a, z) = ([p.x.mid(), p.y.mid()], [q.x.mid(), q.y.mid()]);
                    // the rounding gaps between the exact ends and the segment's ends
                    for end in [p, q] {
                        self.rounding_gap(face, end, open_length, zcap)?;
                    }
                    if a != z {
                        let seg = segment(a, z)?;
                        let len = self.segment_length(face, &seg)?;
                        *open_length = *open_length + len;
                        self.add_piece(face, &seg, 1.0)?;
                    }
                }
                self.add_piece(face, &c, dir)?;
                // the side's sense along the edge's curve on the closed surface: its walk
                // times the face's sign
                let forward = dir * sign > 0.0;
                sides.entry(e).or_default().push((face, c, forward));
            }
        }
        Ok(())
    }

    /// The gap between a curve's exact end (in the box `end`) and the closing segment's end
    /// (its middle): its Green term is at most its extent in u times `int |g| dv` up from
    /// the base, its 3D length at most `|S_u| du + |S_v| dv`, both over the box.
    fn rounding_gap(
        &mut self,
        face: usize,
        end: V3<Ivl>,
        open_length: &mut Ivl,
        zcap: &mut f64,
    ) -> Result<()> {
        let fg = self.faces.get(face).ok_or_else(|| SolidError::sm_err(OP))?;
        let p = partials_at(&fg.cn, end.x, end.y, 1)?;
        let (s, su, sv) = (p.at(0, 0), p.at(1, 0), p.at(0, 1));
        let (du, dv) = (width(end.x), width(end.y));
        *zcap = zcap.max((s.z - Ivl::from(self.z0)).mag());
        *open_length = *open_length
            + Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(du)
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(dv);
        let base = fg.levels.first().copied().unwrap_or(end.y.lo);
        let g = g_integral(&fg.cn, end.x, base, end.y.hi, self.z0)?;
        let green = Ivl::from(du) * Ivl::from(g);
        self.fixed = self.fixed + Ivl::new(-green.hi, green.hi);
        Ok(())
    }

    /// A bound on the 3D length of a closing segment on face `face`: along a side of the net
    /// whose control points are nearly one point (a pole), from their spread
    /// ([`side_length`]); else from the partials over its box.
    fn segment_length(&self, face: usize, seg: &NurbsCurve) -> Result<Ivl> {
        let fg = self.faces.get(face).ok_or_else(|| SolidError::sm_err(OP))?;
        let (u, v) = piece_hull(seg, seg.p);
        if let Some(len) = side_length(&fg.cn, u, v) {
            return Ok(len);
        }
        let p = partials_at(&fg.cn, u, v, 1)?;
        let (su, sv) = (p.at(1, 0), p.at(0, 1));
        Ok(Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(width(u))
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(width(v)))
    }

    /// Adds the cells of one pcurve on face `face`: each polynomial piece, cut where it may
    /// cross a knot line of the surface, swept down to the base.
    fn add_piece(&mut self, face: usize, cn: &NurbsCurve, dir: f64) -> Result<()> {
        let fg = self.faces.get(face).ok_or_else(|| SolidError::sm_err(OP))?;
        let mut lines_u: Vec<f64> = Vec::new();
        for &x in &fg.cn.u {
            if lines_u.last() != Some(&x) {
                lines_u.push(x);
            }
        }
        let levels = fg.levels.clone();
        let lines_v: Vec<f64> = levels.iter().skip(1).copied().collect();
        for (s, a, z) in curve_spans(cn) {
            let mut cuts = vec![a, z];
            for (c, lines) in [(0usize, &lines_u), (1, &lines_v)] {
                for &line in lines {
                    crossings(cn, s, a, z, c, line, &mut cuts)?;
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup();
            let piece = self.pieces.len();
            self.pieces.push(Piece {
                face,
                cn: cn.clone(),
                span: s,
                dir,
            });
            for pair in cuts.windows(2) {
                let &[t0, t1] = pair else {
                    continue;
                };
                // the decomposition below the trim, by where its middle lies: the whole
                // spans under it and the part of the span it is in
                let mid = curve_derivs_over(cn, Ivl::from(0.5 * (t0 + t1)), 0, Some(s))?;
                let vc = mid.first().map_or(f64::NAN, |p| p.y.mid());
                let j_top = levels.iter().rposition(|&l| l <= vc).unwrap_or(0);
                for j in 0..=j_top {
                    self.push(Item {
                        kind: Kind::Sweep {
                            piece,
                            j,
                            whole: j < j_top,
                        },
                        t: (t0, t1),
                        s: (0.0, 1.0),
                        depth: 0,
                        order: FIRST_ORDER,
                        settled: false,
                        enc: IZ,
                        split_t: true,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn add_strip(
        &mut self,
        a: (usize, NurbsCurve),
        c: (usize, NurbsCurve),
        open_length: &mut Ivl,
        zcap: &mut f64,
    ) -> Result<()> {
        let range = |cn: &NurbsCurve| (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.n as isize));
        let (ra, rc) = (range(&a.1), range(&c.1));
        let (t0, t1) = (ra.0.max(rc.0), ra.1.min(rc.1));
        if t0.partial_cmp(&t1) != Some(core::cmp::Ordering::Less) {
            return Err(SolidError::invalid_input(
                OP,
                "an edge whose two pcurves share no parameters",
            ));
        }
        let strip = self.strips.len();
        self.strips.push(StripGeo { sides: [a, c] });
        // the parts of either curve past the common parameters, and the strip's two ends
        for side in 0..2 {
            let (lo, hi) = if side == 0 { ra } else { rc };
            for (x0, x1) in [(lo, t0), (t1, hi)] {
                if x0 < x1 {
                    let len = self.curve_length(strip, side, x0, x1)?;
                    *open_length = *open_length + len;
                }
            }
        }
        for t in [t0, t1] {
            let (x, y) = (self.point(strip, 0, t)?, self.point(strip, 1, t)?);
            let d = [y[0] - x[0], y[1] - x[1], y[2] - x[2]];
            *open_length = *open_length + Ivl::from(mag3(d));
            *zcap = zcap.max((x[2] - Ivl::from(self.z0)).mag());
        }
        // cut at both curves' knots
        let sg = self
            .strips
            .get(strip)
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let mut cuts = vec![t0, t1];
        for (_, cn) in &sg.sides {
            for &x in &cn.u {
                if x > t0 && x < t1 {
                    cuts.push(x);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let spans_at = |t: f64| -> [usize; 2] {
            let s = |cn: &NurbsCurve| span(&cn.u, cn.p, cn.n, t);
            [s(&sg.sides[0].1), s(&sg.sides[1].1)]
        };
        let mut items = Vec::new();
        for pair in cuts.windows(2) {
            if let &[x0, x1] = pair {
                items.push(Item {
                    kind: Kind::Strip {
                        strip,
                        spans: spans_at(0.5 * (x0 + x1)),
                    },
                    t: (x0, x1),
                    s: (0.0, 1.0),
                    depth: 0,
                    order: FIRST_ORDER,
                    settled: false,
                    enc: IZ,
                    split_t: true,
                });
            }
        }
        for it in items {
            self.push(it)?;
        }
        Ok(())
    }

    /// The 3D point of a strip's side `side` at `t`, enclosed.
    fn point(&self, strip: usize, side: usize, t: f64) -> Result<[Ivl; 3]> {
        let (face, cn) = self.strip_side(strip, side)?;
        let d = curve_derivs_over(cn, Ivl::from(t), 0, None)?;
        let p = d.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
        let s = partials_at(&face.cn, p.x, p.y, 0)?.at(0, 0);
        Ok([s.x, s.y, s.z])
    }

    /// A bound on the 3D length of a strip side's curve over `[t0, t1]`.
    fn curve_length(&self, strip: usize, side: usize, t0: f64, t1: f64) -> Result<Ivl> {
        let (face, cn) = self.strip_side(strip, side)?;
        let d = curve_derivs_over(cn, Ivl::new(t0, t1), 1, None)?;
        let (p, dp) = (
            d.first().copied().ok_or_else(|| SolidError::sm_err(OP))?,
            d.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?,
        );
        let s = partials_at(&face.cn, p.x, p.y, 1)?;
        let (su, sv) = (s.at(1, 0), s.at(0, 1));
        let speed = Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(dp.x.mag())
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(dp.y.mag());
        Ok(speed * (Ivl::from(t1) - Ivl::from(t0)))
    }

    fn strip_side(&self, strip: usize, side: usize) -> Result<(&FaceGeo, &NurbsCurve)> {
        let (face, cn) = self
            .strips
            .get(strip)
            .and_then(|s| s.sides.get(side))
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let fg = self
            .faces
            .get(*face)
            .ok_or_else(|| SolidError::sm_err(OP))?;
        Ok((fg, cn))
    }

    /// Evaluates an item and puts it on the work list.
    fn push(&mut self, mut it: Item) -> Result<()> {
        self.counts.cells += 1;
        if self.counts.cells % 64 == 0 {
            self.k.check_cancelled(OP)?;
        }
        let (enc, split_t) = match it.kind {
            Kind::Sweep { piece, j, whole } => {
                self.sweep_cell(piece, j, whole, it.t, it.s, it.order)?
            }
            Kind::Strip { strip, spans } => {
                self.counts.strips += 1;
                let (enc, settled) = self.strip_cell(strip, spans, it.t, it.order)?;
                it.settled = settled;
                (enc, true)
            }
        };
        it.enc = enc;
        it.split_t = split_t;
        let index = self.items.len();
        let width = enc.hi - enc.lo;
        let key = if width >= 0.0 { width } else { f64::INFINITY };
        self.items.push(it);
        self.alive.push(true);
        self.heap.push((key.to_bits(), Reverse(index)));
        Ok(())
    }

    /// A sweep cell's enclosure, signed, and whether it is to be cut along `t`.
    fn sweep_cell(
        &mut self,
        piece: usize,
        j: usize,
        whole: bool,
        t: (f64, f64),
        s: (f64, f64),
        ord: usize,
    ) -> Result<(Ivl, bool)> {
        let p = self
            .pieces
            .get(piece)
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let fg = self
            .faces
            .get(p.face)
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let sign = Ivl::from(fg.sign * p.dir);
        let a = fg
            .levels
            .get(j)
            .copied()
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let next = fg.levels.get(j + 1).copied();
        let z0 = Ivl::from(self.z0);
        let (tc, sc) = (0.5 * (t.0 + t.1), 0.5 * (s.0 + s.1));
        let (xl, xh) = (
            Ivl::from(t.0) - Ivl::from(tc),
            Ivl::from(t.1) - Ivl::from(tc),
        );
        let (yl, yh) = (
            Ivl::from(s.0) - Ivl::from(sc),
            Ivl::from(s.1) - Ivl::from(sc),
        );
        let (hull_u, hull_v) = piece_hull(&p.cn, p.span);
        // the series of u and of b (v = a + sigma b) at `tt` (a point or the cell's t), and
        // the box the points map into
        let series = |tt: Ivl, sg: Ivl, n: usize| -> Result<(Bs, Bs, Ivl, Ivl)> {
            let d = curve_derivs_over(&p.cn, tt, n, Some(p.span))?;
            let (u0, du) = curve_series(&d, 0, n);
            let (v0, vs) = curve_series(&d, 1, n);
            let (b0, bs) = match (whole, next) {
                (true, Some(top)) => (Ivl::from(top) - Ivl::from(a), Bs::zero()),
                (true, None) => return Err(SolidError::sm_err(OP)),
                (false, _) => (v0 - Ivl::from(a), vs),
            };
            // dv = sg (b(x) - b0) + y b(x)
            let mut dv = bs.scale(sg);
            dv.set(0, 1, b0);
            for kk in 1..n {
                dv.set(kk, 1, bs.at(kk, 0));
            }
            // where the points lie: the enclosure, cut to the curve's hull
            let u = Ivl::new(u0.lo.max(hull_u.lo), u0.hi.min(hull_u.hi));
            let vb = Ivl::from(a) + sg * b0;
            let true_v = match (whole, next) {
                (true, Some(top)) => Ivl::new(a, top),
                _ => Ivl::new(a.min(hull_v.lo), a.max(hull_v.hi)),
            };
            let v = Ivl::new(vb.lo.max(true_v.lo), vb.hi.min(true_v.hi));
            Ok((du, dv, u, v))
        };
        // the integrand -(S_z - z0) (S_u x S_v)_z u'(t) b(t) as a series of degree `n` (the
        // Jacobian of phi is u' b): in this form no product cancels another, where the
        // equal form (X_t x X_sigma)_z of the composed surface does, and an interval keeps
        // a cancellation's whole width (from partials to order n + 1, du and dv to n + 1)
        let integrand = |p: &Partials<Ivl>, du: &Bs, dv: &Bs, n: usize| -> Bs {
            let [_, _, xz] = compose_partial(p, (0, 0), du, dv, n);
            let [sux, suy, _] = compose_partial(p, (1, 0), du, dv, n);
            let [svx, svy, _] = compose_partial(p, (0, 1), du, dv, n);
            let normal = sux.mul(&svy, n).sub(&suy.mul(&svx, n));
            let g = xz.sub(&Bs::constant(z0)).mul(&normal, n);
            let mut b = Bs::zero();
            for kk in 0..=n {
                b.set(kk, 0, dv.at(kk, 1));
            }
            g.mul(&du.dx(n + 1), n).mul(&b, n).scale(Ivl::from(-1.0))
        };
        let tcell = Ivl::new(t.0, t.1);
        let scell = Ivl::new(s.0, s.1);
        let (du_c, dv_c, u_c, v_c) = series(tcell, scell, ord + 1)?;
        let taylor = piece_of(&fg.cn, u_c, v_c);
        let Some((ku, kv)) = taylor else {
            // the range over the cell times its area
            self.counts.ranged += 1;
            let (du1, dv1, u1, v1) = series(tcell, scell, 1)?;
            let Some(pr) = partials_over(&fg.cn, u1, v1, 1)? else {
                return Ok((Ivl::whole(), true));
            };
            let x = compose(&pr, &du1, &dv1, 1);
            let f = integrand(&pr, &du1, &dv1, 0).at(0, 0);
            let area = (xh - xl) * (yh - yl);
            let [xx, xy, _] = &x;
            let wt = (xh - xl).mag() * (mag3([xx.at(1, 0), xy.at(1, 0), IZ]));
            let ws = (yh - yl).mag() * (mag3([xx.at(0, 1), xy.at(0, 1), IZ]));
            let crosses_u = piece_of(&fg.cn, u1, Ivl::from(v1.mid())).is_none();
            return Ok((sign * f * area, crosses_u || wt >= ws));
        };
        self.counts.taylor += 1;
        // the remainder: the coefficients of degree ORDER over the cell
        let (du_m, dv_m, u_m, v_m) = series(Ivl::from(tc), Ivl::from(sc), ord)?;
        let (Some(pr), Some(pm)) = (
            piece_partials(&fg.cn, (ku, kv), u_c, v_c, ord + 1),
            piece_partials(&fg.cn, (ku, kv), u_m, v_m, ord),
        ) else {
            return Ok((Ivl::whole(), true));
        };
        let fr = integrand(&pr, &du_c, &dv_c, ord);
        // the polynomial: the coefficients of degree < ORDER at the middle
        let fm = integrand(&pm, &du_m, &dv_m, ord - 1);
        let mx: Vec<Ivl> = (0..=ord).map(|i| moment(xl, xh, i)).collect();
        let my: Vec<Ivl> = (0..=ord).map(|i| moment(yl, yh, i)).collect();
        let ax: Vec<Ivl> = (0..=ord).map(|i| abs_moment(xl, xh, i)).collect();
        let ay: Vec<Ivl> = (0..=ord).map(|i| abs_moment(yl, yh, i)).collect();
        let at = |v: &[Ivl], i: usize| v.get(i).copied().unwrap_or(Ivl::whole());
        let mut sum = Ivl::from(0.0);
        for d in 0..ord {
            for jj in 0..=d {
                let c = fm.at(d - jj, jj);
                if !is_zero(c) {
                    sum = sum + c * at(&mx, d - jj) * at(&my, jj);
                }
            }
        }
        let (mut wt, mut ws) = (0.0_f64, 0.0_f64);
        for jj in 0..=ord {
            let i = ord - jj;
            let c = fr.at(i, jj);
            if is_zero(c) {
                continue;
            }
            let m = at(&ax, i) * at(&ay, jj);
            // x^i y^j keeps its sign over the cell where both powers are even
            let r = if i % 2 == 0 && jj % 2 == 0 {
                c * m
            } else {
                Ivl::new(-c.mag(), c.mag()) * m
            };
            if i > jj {
                wt += r.hi - r.lo;
            } else if jj > i {
                ws += r.hi - r.lo;
            }
            sum = sum + r;
        }
        Ok((sign * sum, wt >= ws))
    }

    /// A strip cell's enclosure, `[-w, w]` with `w` its flux's bound, and whether it is
    /// settled: at least half of `int |D|` is the gap's least size over the cell (at its
    /// middle, less the slope's bound times the half length) times its length, which every
    /// cut of it keeps, so cutting it can at most halve that factor. A
    /// real gap between two faces' boundaries (not their rounding) is such a cell: refining
    /// it would spend the budget without deciding.
    fn strip_cell(
        &self,
        strip: usize,
        spans: [usize; 2],
        t: (f64, f64),
        ord: usize,
    ) -> Result<(Ivl, bool)> {
        let tc = 0.5 * (t.0 + t.1);
        let (xl, xh) = (
            Ivl::from(t.0) - Ivl::from(tc),
            Ivl::from(t.1) - Ivl::from(tc),
        );
        let z0 = Ivl::from(self.z0);
        // each side's curve in 3D as a series in x: at the middle (degree < ORDER) and over
        // the cell (degree ORDER), or its range over the cell where a box may cross a knot
        // line
        let mut mid = Vec::new();
        let mut over = Vec::new();
        let mut taylor = true;
        for side in 0..2 {
            let (face, cn) = self.strip_side(strip, side)?;
            let sp = spans
                .get(side)
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let (hu, hv) = piece_hull(cn, sp);
            let at = |tt: Ivl, n: usize| -> Result<(Bs, Bs, Ivl, Ivl)> {
                let d = curve_derivs_over(cn, tt, n, Some(sp))?;
                let (u0, du) = curve_series(&d, 0, n);
                let (v0, dv) = curve_series(&d, 1, n);
                let u = Ivl::new(u0.lo.max(hu.lo), u0.hi.min(hu.hi));
                let v = Ivl::new(v0.lo.max(hv.lo), v0.hi.min(hv.hi));
                Ok((du, dv, u, v))
            };
            let (du, dv, u, v) = at(Ivl::new(t.0, t.1), ord)?;
            let (dum, dvm, um, vm) = at(Ivl::from(tc), ord - 1)?;
            let pieces = piece_of(&face.cn, u, v).and_then(|kk| {
                Some((
                    piece_partials(&face.cn, kk, u, v, ord)?,
                    piece_partials(&face.cn, kk, um, vm, ord - 1)?,
                ))
            });
            match pieces {
                Some((po, pm)) if taylor => {
                    over.push(compose(&po, &du, &dv, ord));
                    mid.push(compose(&pm, &dum, &dvm, ord - 1));
                }
                _ => {
                    taylor = false;
                    let (du, dv, u, v) = at(Ivl::new(t.0, t.1), 1)?;
                    let Some(p) = partials_over(&face.cn, u, v, 1)? else {
                        return Ok((Ivl::whole(), false));
                    };
                    over.push(compose(&p, &du, &dv, 1));
                }
            }
        }
        let (Some(o1), Some(o2)) = (over.first(), over.get(1)) else {
            return Err(SolidError::sm_err(OP));
        };
        let comp = |s: &[Bs; 3], i: usize| [s[0].at(i, 0), s[1].at(i, 0), s[2].at(i, 0)];
        let diff = |a: [Ivl; 3], b: [Ivl; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        // |X_1'| + |D'| and max |z - z0| over the cell
        let speed = mag3([
            Ivl::from(mag3(comp(o1, 1))),
            Ivl::from(mag3(diff(comp(o1, 1), comp(o2, 1)))),
            IZ,
        ]);
        let zmax = (o1[2].at(0, 0) - z0).mag().max((o2[2].at(0, 0) - z0).mag());
        // int |D| over the cell
        let gap = match (taylor, mid.first(), mid.get(1)) {
            (true, Some(m1), Some(m2)) => {
                let mut g = Ivl::from(0.0);
                for i in 0..ord {
                    let d = mag3(diff(comp(m1, i), comp(m2, i)));
                    g = g + Ivl::from(d) * abs_moment(xl, xh, i);
                }
                let d = mag3(diff(comp(o1, ord), comp(o2, ord)));
                g + Ivl::from(d) * abs_moment(xl, xh, ord)
            }
            _ => Ivl::from(mag3(diff(comp(o1, 0), comp(o2, 0)))) * (xh - xl),
        };
        let w = Ivl::from(zmax) * Ivl::from(speed) * gap;
        // the gap's least size over the cell, in the norm `int |D|` is taken in (the sum of
        // the components, `mag3`): at the middle less its derivative's bound times the half
        // length, or over the whole cell where there is no middle series
        let least_sum = |d: [Ivl; 3]| {
            d.iter()
                .fold(Ivl::from(0.0), |sum, c| sum + Ivl::from(c.mig()))
                .lo
        };
        let least = match (taylor, mid.first(), mid.get(1)) {
            (true, Some(m1), Some(m2)) => {
                let slope = mag3(diff(comp(o1, 1), comp(o2, 1)));
                let half = xl.mag().max(xh.mag());
                (Ivl::from(least_sum(diff(comp(m1, 0), comp(m2, 0))))
                    - Ivl::from(slope) * Ivl::from(half))
                .lo
                .max(0.0)
            }
            _ => least_sum(diff(comp(o1, 0), comp(o2, 0))),
        };
        let kept = (Ivl::from(least) * (xh - xl)).lo;
        let settled = kept + kept >= gap.hi;
        Ok((Ivl::new(-w.hi, w.hi), settled))
    }

    /// The enclosure: the cells' sum and the fixed terms, in insertion order.
    fn total(&self) -> Ivl {
        let mut sum = self.fixed;
        for (it, alive) in self.items.iter().zip(&self.alive) {
            if *alive {
                sum = sum + it.enc;
            }
        }
        sum
    }

    /// The enclosure in two parts: the sum of the cells still refined, and the half width
    /// of the rest (the settled strip cells and the fixed terms, each `[-w, w]`), rounded
    /// up.
    fn parts(&self) -> (Ivl, f64) {
        let mut rest = Ivl::from(0.0);
        let mut still = Ivl::from(self.fixed.hi);
        for (it, alive) in self.items.iter().zip(&self.alive) {
            if !*alive {
                continue;
            }
            if it.settled {
                still = still + Ivl::from(it.enc.hi);
            } else {
                rest = rest + it.enc;
            }
        }
        (rest, still.hi)
    }

    /// Refines the widest cell, one bisection at a time, until `done` holds of the
    /// enclosure, `hopeless` of its parts ([`Work::parts`]: no refining can make `done`
    /// hold), or the budget is spent.
    fn refine(
        &mut self,
        done: impl Fn(Ivl) -> bool,
        hopeless: impl Fn(Ivl, f64) -> bool,
    ) -> Result<VolumeEnclosure> {
        let out = |w: &Self| {
            let e = w.total();
            VolumeEnclosure {
                lo: e.lo,
                hi: e.hi,
                counts: w.counts,
            }
        };
        // running sums in f64 say when to sum the enclosure in intervals again: when they
        // look done, or every quarter of the work list's length
        let total = self.total();
        let (mut lo, mut hi) = (total.lo, total.hi);
        let mut checked = self.counts.cells;
        let mut next_check = 0usize;
        loop {
            let due = self.counts.cells >= next_check
                || (done(Ivl::new(lo, hi)) && self.counts.cells >= checked + 16);
            if due {
                let total = self.total();
                if done(total) {
                    return Ok(out(self));
                }
                let (rest, still) = self.parts();
                if hopeless(rest, still) {
                    return Ok(out(self));
                }
                (lo, hi) = (total.lo, total.hi);
                checked = self.counts.cells;
                next_check = self.counts.cells + (self.items.len() / 4).max(64);
            }
            if self.counts.cells >= self.budget {
                return Ok(out(self));
            }
            let Some((_, Reverse(i))) = self.heap.pop() else {
                return Ok(out(self));
            };
            let Some(it) = self.items.get(i).copied() else {
                continue;
            };
            if it.settled {
                continue; // a gap no cut narrows: kept as it is
            }
            if it.order < ORDER {
                // evaluated again at the full order before it is cut
                if let Some(a) = self.alive.get_mut(i) {
                    *a = false;
                }
                lo -= it.enc.lo;
                hi -= it.enc.hi;
                self.push(Item { order: ORDER, ..it })?;
                if let Some(n) = self.items.last() {
                    lo += n.enc.lo;
                    hi += n.enc.hi;
                }
                continue;
            }
            if it.depth >= MAX_DEPTH {
                continue; // kept as it is
            }
            let tm = 0.5 * (it.t.0 + it.t.1);
            let sm = 0.5 * (it.s.0 + it.s.1);
            let can_t = it.t.0 < tm && tm < it.t.1;
            let can_s = it.s.0 < sm && sm < it.s.1 && matches!(it.kind, Kind::Sweep { .. });
            // an unbounded cell (weights not proved positive over its box) is cut along t
            // and sigma in turn: its box shrinks in both, whichever its size depends on
            let split_t = if (it.enc.hi - it.enc.lo).is_finite() {
                it.split_t
            } else {
                it.depth % 2 == 0
            };
            let children = if can_t && (split_t || !can_s) {
                [(it.t.0, tm), (tm, it.t.1)].map(|t| Item {
                    t,
                    depth: it.depth + 1,
                    ..it
                })
            } else if can_s {
                [(it.s.0, sm), (sm, it.s.1)].map(|s| Item {
                    s,
                    depth: it.depth + 1,
                    ..it
                })
            } else {
                continue; // a cell of single parameters: kept as it is
            };
            if let Some(a) = self.alive.get_mut(i) {
                *a = false;
            }
            lo -= it.enc.lo;
            hi -= it.enc.hi;
            for c in children {
                self.push(c)?;
                if let Some(n) = self.items.last() {
                    lo += n.enc.lo;
                    hi += n.enc.hi;
                }
            }
        }
    }
}

/// The parameters in `(a, z)` where the curve's coordinate `c` (0: u, 1: v) on span `s`
/// crosses `line`, located by sign changes between 16 evenly spaced points and bisection
/// (where the cells are cut; a cut off the exact crossing only leaves a cell whose image
/// may cross the line, enclosed by its range).
fn crossings(
    cn: &NurbsCurve,
    s: usize,
    a: f64,
    z: f64,
    c: usize,
    line: f64,
    out: &mut Vec<f64>,
) -> Result<()> {
    const SAMPLES: usize = 16;
    let (hu, hv) = piece_hull(cn, s);
    let h = if c == 0 { hu } else { hv };
    if !(h.lo < line && line < h.hi) {
        return Ok(());
    }
    let at = |t: f64| -> Result<f64> {
        let d = curve_derivs_over(cn, Ivl::from(t), 0, Some(s))?;
        let p = d.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
        Ok((if c == 0 { p.x } else { p.y }).mid() - line)
    };
    let mut t0 = a;
    let mut f0 = at(t0)?;
    for i in 1..=SAMPLES {
        let t1 = a + (z - a) * (i as f64) / (SAMPLES as f64);
        let f1 = at(t1)?;
        if f0 * f1 < 0.0 {
            let (mut lo, mut hi, mut flo) = (t0, t1, f0);
            for _ in 0..64 {
                let m = 0.5 * (lo + hi);
                if !(lo < m && m < hi) {
                    break;
                }
                let fm = at(m)?;
                if fm * flo <= 0.0 {
                    hi = m;
                } else {
                    lo = m;
                    flo = fm;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        t0 = t1;
        f0 = f1;
    }
    Ok(())
}

/// A bound on the 3D length of the uv box where it lies on the net's first or last knot in
/// one direction (`None` elsewhere). On that side the surface is the rational curve of the
/// side's control points `P_i`: with them in a box of diameter `delta`, its derivative
/// `sum (N_i w_i)' (P_i - C) / W` is at most `sum |N_i'| max w delta / min w`, and
/// `sum |N_i'| <= 2 p / h` on a knot span of length `h` (each `N_i'` is two terms of
/// `p / (u_(i+p) - u_i)` times a basis function of degree `p - 1`). At a pole, where the
/// points are one point to rounding, the bound is of the order of rounding. A box off the
/// side by rounding (a trim's end a few units in the last place from the knot) adds its
/// distance times the mixed partial over the box between.
fn side_length(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<Ivl> {
    let (u, v, across) = on_side(cn, u, v)?;
    let along = side_speed_bound(cn, u, v)?;
    // off the side by `d` (rounding): the speed along it grows by at most `d` times the
    // mixed partial over the box between
    let between = |x: Ivl, k: f64| Ivl::new(x.lo.min(k), x.hi.max(k));
    let (bu, bv, off, extent) = match across {
        Across::V(k) => (u, between(v, k), (v, k), width(u)),
        Across::U(k) => (between(u, k), v, (u, k), width(v)),
    };
    let d = (off.0.lo - off.1).abs().max((off.0.hi - off.1).abs());
    let mixed = if d > 0.0 {
        // (`None` for a box past the finite numbers or weights not proved positive: the
        // caller then bounds the length by the partials over the box)
        let p = partials_over(cn, bu, bv, 2).ok()??.at(1, 1);
        Ivl::from(mag3([p.x, p.y, p.z])) * Ivl::from(d) * Ivl::from(extent)
    } else {
        Ivl::from(0.0)
    };
    Some(along + mixed)
}

/// Which side of the net a box lies on to within rounding (`1e-12` of the knot's size):
/// the box, and the side's knot in the direction across it.
fn on_side(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<(Ivl, Ivl, Across)> {
    let near = |x: Ivl, k: f64| {
        let tol = 1e-12 * (1.0 + k.abs());
        (x.lo - k).abs() <= tol && (x.hi - k).abs() <= tol
    };
    let (v0, v1) = (knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize));
    let (u0, u1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize));
    let side = [(v, v0, true), (v, v1, true), (u, u0, false), (u, u1, false)]
        .into_iter()
        .find(|&(x, k, _)| near(x, k))?;
    Some((
        u,
        v,
        if side.2 {
            Across::V(side.1)
        } else {
            Across::U(side.1)
        },
    ))
}

/// The direction across a side of the net, with the side's knot.
#[derive(Clone, Copy)]
enum Across {
    /// The side is the u line at this v knot.
    V(f64),
    /// The side is the v line at this u knot.
    U(f64),
}

/// The bound of [`side_length`] on the side itself.
fn side_speed_bound(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<Ivl> {
    let point = |i: usize, j: usize| -> Option<([Ivl; 3], f64)> {
        let at = |c: usize| cn.pw.get(4 * (i * cn.nv + j) + c).copied();
        let w = at(3)?;
        let d = |c: usize| at(c).map(|x| Ivl::from(x) / Ivl::from(w));
        Some(([d(0)?, d(1)?, d(2)?], w))
    };
    let (_, _, across) = on_side(cn, u, v)?;
    let (v0, v1) = (knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize));
    let (u0, u1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize));
    // the side's control points, and the direction along it: its knots, degree, count and
    // the box's range
    let on = |k: f64| match across {
        Across::V(x) | Across::U(x) => x == k,
    };
    let (pts, knots, p, n, along): (Vec<_>, &[f64], usize, usize, Ivl) =
        if matches!(across, Across::V(_)) && on(v0) {
            (
                (0..cn.nu).map(|i| point(i, 0)).collect(),
                &cn.u,
                cn.p,
                cn.nu,
                u,
            )
        } else if matches!(across, Across::V(_)) && on(v1) {
            (
                (0..cn.nu)
                    .map(|i| point(i, cn.nv.saturating_sub(1)))
                    .collect(),
                &cn.u,
                cn.p,
                cn.nu,
                u,
            )
        } else if matches!(across, Across::U(_)) && on(u0) {
            (
                (0..cn.nv).map(|j| point(0, j)).collect(),
                &cn.v,
                cn.q,
                cn.nv,
                v,
            )
        } else if matches!(across, Across::U(_)) && on(u1) {
            (
                (0..cn.nv)
                    .map(|j| point(cn.nu.saturating_sub(1), j))
                    .collect(),
                &cn.v,
                cn.q,
                cn.nv,
                v,
            )
        } else {
            return None;
        };
    let mut hull: Option<[Ivl; 3]> = None;
    let (mut wmin, mut wmax) = (f64::INFINITY, 0.0_f64);
    for q in pts {
        let (x, w) = q?;
        hull = Some(match hull {
            None => x,
            Some(h) => [h[0].hull(x[0]), h[1].hull(x[1]), h[2].hull(x[2])],
        });
        wmin = wmin.min(w);
        wmax = wmax.max(w);
    }
    let h = hull?;
    let delta = mag3([
        Ivl::from(width(h[0])),
        Ivl::from(width(h[1])),
        Ivl::from(width(h[2])),
    ]);
    // the shortest knot span the range meets
    let mut shortest = f64::INFINITY;
    for k in span(knots, p, n, along.lo)..=span(knots, p, n, along.hi) {
        let len = knot(knots, k as isize + 1) - knot(knots, k as isize);
        if len > 0.0 {
            shortest = shortest.min(len);
        }
    }
    let ok = wmin > 0.0 && shortest.is_finite() && delta.is_finite();
    if !ok {
        return None;
    }
    let speed = Ivl::from(2.0 * p as f64) / Ivl::from(shortest) * Ivl::from(wmax) / Ivl::from(wmin)
        * Ivl::from(delta);
    Some(speed * Ivl::from(width(along)))
}

/// The straight uv segment from `a` to `z` as a degree 1 curve on `[0, 1]`.
fn segment(a: [f64; 2], z: [f64; 2]) -> Result<NurbsCurve> {
    let ok = a.iter().chain(&z).all(|x| x.is_finite());
    if !ok {
        return Err(SolidError::invalid_input(
            OP,
            "a loop's end that is not finite",
        ));
    }
    Ok(NurbsCurve {
        p: 1,
        n: 2,
        u: vec![0.0, 0.0, 1.0, 1.0],
        pw: vec![a[0], a[1], 0.0, 1.0, z[0], z[1], 0.0, 1.0],
    })
}

/// An edge use's pcurve as NURBS data, made first where it is missing (as the integrator's
/// trim boundary makes it).
fn pcurve(k: &Kernel, b: &mut Brep, eu: EdgeuseId) -> Result<NurbsCurve> {
    let fetch =
        crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    fetch.status?;
    let c = crate::topology::edgeuse::p1::uv_trim_curve(b, eu)?
        .0
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let bs: &BSplineCurve = c.b_spline_base().ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a pcurve with no NURBS form",
    })?;
    NurbsCurve::of(bs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A product of series is the product of the polynomials, truncated.
    #[test]
    fn series_multiply_and_differentiate_as_polynomials() {
        // (1 + x + y)^2 = 1 + 2x + 2y + x^2 + 2xy + y^2
        let mut a = Bs::constant(Ivl::from(1.0));
        a.set(1, 0, Ivl::from(1.0));
        a.set(0, 1, Ivl::from(1.0));
        let s = a.mul(&a, XORD);
        for (i, j, c) in [
            (0, 0, 1.0),
            (1, 0, 2.0),
            (0, 1, 2.0),
            (2, 0, 1.0),
            (1, 1, 2.0),
            (0, 2, 1.0),
        ] {
            let v = s.at(i, j);
            assert!(v.lo <= c && c <= v.hi, "{i} {j}: {v:?}");
        }
        assert!(is_zero(s.at(3, 0)));
        let d = s.dx(2);
        assert!(d.at(0, 0).lo <= 2.0 && 2.0 <= d.at(0, 0).hi);
        assert!(d.at(0, 1).lo <= 2.0 && 2.0 <= d.at(0, 1).hi);
    }

    /// The bounds' sums and widths round up: 1 + 1e-16 is not a double, and rounded to the
    /// nearest it is 1, below the true sum.
    #[test]
    fn bounds_round_up() {
        assert!(mag3([Ivl::from(1.0), Ivl::from(1e-16), IZ]) > 1.0);
        assert!(width(Ivl::new(-1e-16, 1.0)) > 1.0);
    }

    /// A budget too small to decide leaves the sign undecided, its enclosure holding the
    /// volume (the quarter shell R 1e4, t 1e-3, V 157.08, decides within some 150 cells).
    #[test]
    fn a_spent_budget_leaves_the_sign_undecided() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_cylindrical_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            1e4,
            1e4 + 1e-3,
            0.0,
            90.0,
        )
        .unwrap();
        let v = 157.0796;
        match volume_sign_within(&k, &mut b, 16).unwrap() {
            VolumeSign::Undecided(e) => assert!(e.lo < v && v < e.hi, "{e:?}"),
            other => panic!("{other:?}"),
        }
    }

    /// The moments hold the closed forms.
    #[test]
    fn moments_hold_their_closed_forms() {
        let (xl, xh) = (Ivl::from(-0.5), Ivl::from(1.5));
        // int x^3 over [-0.5, 1.5] = (1.5^4 - 0.5^4) / 4 = 1.25
        let m = moment(xl, xh, 3);
        assert!(m.lo <= 1.25 && 1.25 <= m.hi && m.hi - m.lo < 1e-14);
        // int |x|^3 = (1.5^4 + 0.5^4) / 4 = 1.28125
        let a = abs_moment(xl, xh, 3);
        assert!(a.lo <= 1.28125 && 1.28125 <= a.hi);
    }
}
