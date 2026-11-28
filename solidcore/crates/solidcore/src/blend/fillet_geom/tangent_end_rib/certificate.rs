//! The certificate of a tangent-end rib between its stations (not upstream, #17744).
//!
//! The rib is its definition: at the curves' parameter `t` the section `sigma(t)` of the
//! fillet surface S meets the end surface E, `F(t, y) = S(sigma(t), alpha) − E(u, v) = 0` in
//! the unknowns `y = (alpha, u, v)`. Over a piece `T` of a span, Krawczyk's test
//! (`crate::geom::krawczyk`) on `z = y − c(t)`, `c` the quadratic through the Newton roots at
//! the piece's ends and middle, proves for every `t` in `T` exactly one root `y(t)` in
//! `c(t) + Z`: `F(t, c(t))` is enclosed by its mean-value form about the middle and the
//! partials over `T x (c(T) + Z)`. At a piece's end, a point, the root is in a small box
//! (Krawczyk's at the point, about the same Newton root the piece's quadratic starts from),
//! and that box lies inside `c(t) + Y` there, `Y` the box the piece's test ran on, in which
//! the root at each `t` is the only one, so the end's root is the path's (a piece where it
//! does not is not bounded).
//! Each of the rib's three curves (3D; on the fillet surface's UV; on
//! the end surface's UV) has an error `e` against the root: its 3D point less the rib's
//! point, its UV point less `(sigma, alpha)` or `(u, v)`. At the piece's ends `e` is
//! enclosed from those small boxes; between them `|e(t)|` is at most the larger end's plus
//! `h²/8 max |e''|` (the linear interpolation's remainder), with `e''` enclosed over the
//! piece: the curves' second derivatives over `T`, the root's `y'` and `y''` from
//! differentiating `F = 0` twice, each solved over the box by the preconditioned form
//! `x ∈ C b + (I − C J) x`. A UV curve's error maps to 3D through its surface's first
//! partials over the box both points lie in. Every operation rounds outward.
//!
//! A span is cut into 1, 2, 4, ... up to `2^LEVELS` pieces until every piece is bounded
//! within the tolerance (as `fillet_section::certificate::gn_certify_span` cuts its spans);
//! a piece end already farther than the tolerance from the rib ends the search, since no
//! finer cut can bound the span within it.
//!
//! Each face is enclosed as the surface the kernel evaluates ([`Face`]): NURBS data over
//! every polynomial piece the box covers, hulled: across a knot line inside the box only
//! where the face is at least `C^1` (the remainder needs `e'` continuous), and past the
//! knots on its end piece continued (a box reaches past them only by its rounding at a
//! domain's end); a blend by its definition by the definition's enclosure
//! ([`definition_partials_over_box`], the ball's circle of one radius between NURBS faces),
//! so that the bound is against the definition, with nothing added for its net.
//!
//! The tangent end's span is graded toward it and closed by a bound without the Jacobian
//! ([`Rib::first_span_bound`], [`Rib::near_bound`]: a ring about the tangent end's alpha,
//! and where that costs more than the tolerance, bands about the fillet UV curve's alpha
//! down to where the ring takes over); where that bound is not within the tolerance, the
//! part it covers, `[0, t*]`, carries no claim ([`Bound::Open`]). Every loop over pieces,
//! grades, bands, ring boxes and widenings polls the kernel's cancellation.
//!
//! A rib of a standoff ([`super::Standoff`], [`Rib::standoff`]) is certified the same way
//! with `F(t, y) = S(sigma(t), alpha) + lambda(t) n − E(u, v)`: `lambda` is a known
//! polynomial in `t`, so `F`, its derivatives in `t` and the rib's point (the end surface's
//! less `lambda n`) gain its terms, enclosed over the piece, and the Jacobian is unchanged.
//! Near the tangent end the separation is compared with `lambda` at its section, not 0.

use core::cell::RefCell;

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, knot_cuts, nurbs_partials_over_piece, nurbs_partials_over_piece_about,
    partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::definition_box::{
    definition_enclosable, definition_partials_over_box, gn_def_face_of, GnDefFace,
};
use crate::blend::fillet_section::interval::{cmax, Ivl};
use crate::blend::fillet_section::series::{v_add, v_scale, v_sub, V3};
use crate::blend::fillet_section::station::{as_b_spline, knot, span, Nurbs, Partials};
use crate::error::Result;
use crate::geom::krawczyk::{
    contraction, inverse_of_columns3, krawczyk, krawczyk_region, newton, plus_c_times, Linearised,
};
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
use crate::surface::surface::{Surface, SurfaceKind};

/// The continuity a face needs across a knot line inside a box for its second partials:
/// `C^1`, so that the root and the errors are `C^1` and the remainder holds with `e''`
/// enclosed on each side. Its points and first partials are enclosed across any line
/// (Krawczyk's mean-value form and the centred form need them continuous only).
const CONTINUITY: usize = 1;

/// The most times a piece of a span is halved.
const LEVELS: u32 = 6;

/// The most pieces the first span is graded into toward the tangent end.
const GRADES: usize = 64;

/// The most boxes each side of the ring about the tangent end is cut into
/// ([`Rib::one_sign`]).
const RING_BOXES: usize = 1 << 12;

/// The most times the ring about the tangent end is widened ([`Rib::near_bound`]).
const WIDENINGS: usize = 24;

/// The most pieces of a face one enclosure covers ([`cells`]): a box over more is not
/// enclosed.
const CELLS: usize = 256;

const ZERO: Ivl = Ivl::new(0.0, 0.0);

const OP: &str = "blend::fillet_geom::tangent_end_rib::certificate";

/// A Newton root of the rib `(alpha, u, v)` at `t` from a guess, or `None`.
pub(super) type Solve<'a> = dyn Fn(f64, [f64; 3]) -> Result<Option<[f64; 3]>> + 'a;

/// What the certificate says of a span ([`Rib::span_bound`], [`Rib::first_span_bound`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Bound {
    /// Certified: the largest distance of the curves from the rib, within the tolerance.
    Within(f64),
    /// A piece end whose error's enclosure (over Krawczyk's box there) is over the
    /// tolerance, or a piece bounded only over it after the last halving: the curves are
    /// not proved within it, and more stations move them.
    Over,
    /// A piece not bounded at all: the rib's root, its path or a derivative not enclosed
    /// (Krawczyk's test does not contract), or an error not enclosed over the box the
    /// curves' errors widen.
    Unenclosed,
    /// The first span only: at least one graded piece is certified, the pieces stop at
    /// `t*` (a piece not bounded at all, no Newton root, or the last of [`GRADES`]), and
    /// `[0, t*]` is not bounded within the tolerance. `[0, t*]` carries no claim (the
    /// maintainer's answer on #18674).
    Open,
}

/// A face of the rib as the certificate encloses it: the surface the kernel evaluates.
pub(super) enum Face<'f> {
    /// NURBS data: a NURBS face, a face that is one (plane, revolution, sphere, ...), or a
    /// blend inside a scope that answers by its net. `past_knots`: whether the face is
    /// evaluated past its knots as its end piece continued; a plane clamps its parameters
    /// to its domain, its net's knots, so a box past them is not enclosed.
    Nurbs { net: Nurbs, past_knots: bool },
    /// A blend evaluated by its definition, enclosed by the definition
    /// ([`definition_enclosable`]); its net only seeds the enclosure's feet.
    Definition {
        kernel: &'f Kernel,
        net: Nurbs,
        def: Box<RefCell<GnDefFace<'f>>>,
    },
}

impl<'f> Face<'f> {
    /// The face `s` as the certificate encloses it, or `None` where it is not enclosed (a
    /// STEP, offset, curve-bounded or in-volume face; a cone or cylinder, whose point the
    /// kernel takes from its STEP form, not from its net; a B-spline face in power-basis
    /// form; a blend by its definition that [`definition_enclosable`] does not take, such
    /// as one of contact order 2): the rib is then built without a bound (the maintainer's
    /// design on #18674).
    ///
    /// # Errors
    ///
    /// Those of the face's NURBS data ([`as_b_spline`]); a cancellation.
    pub(super) fn of(k: &'f Kernel, s: &'f Surface) -> Result<Option<Self>> {
        // a face in power-basis form is evaluated from its coefficients, not as a B-spline
        // net: not enclosed
        let net = || -> Result<Option<Nurbs>> {
            Ok(as_b_spline(k, s)?
                .filter(|b| b.form() != BSplineSurfaceForm::Polynomial)
                .and_then(|b| Nurbs::of(&b).ok()))
        };
        let past_knots = match s.kind() {
            // the point from the STEP form, at a parameter converted from the net's (not
            // always in closed form) and clamped to its domain
            SurfaceKind::Cone(_) => return Ok(None),
            SurfaceKind::Plane(_) => false,
            _ => true,
        };
        let nurbs = |net| Face::Nurbs { net, past_knots };
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            return Ok(net()?.map(nurbs));
        };
        if pb.net_answers(k) {
            return Ok(net()?.map(nurbs));
        }
        if !definition_enclosable(s) {
            return Ok(None);
        }
        let def = match gn_def_face_of(k, s) {
            Ok(d) => d,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        };
        Ok(net()?.map(|net| Face::Definition {
            kernel: k,
            net,
            def: Box::new(RefCell::new(def)),
        }))
    }
}

/// The parts of `[lo, hi]` on the pieces of a knot vector: inside the knots cut at the
/// knot lines strictly inside ([`knot_cuts`]), past them the end piece continued (`None`
/// there without `past_knots`, or for more than [`CELLS`] parts).
fn cuts(
    knots: &[f64],
    p: usize,
    n: usize,
    (lo, hi): (f64, f64),
    continuity: usize,
    past_knots: bool,
) -> Option<Vec<(f64, f64, usize)>> {
    let first = knot(knots, isize::try_from(p).ok()?);
    let last = knot(knots, isize::try_from(n).ok()?);
    if !past_knots && (lo < first || hi > last) {
        return None;
    }
    let mut out = Vec::new();
    if lo < first {
        out.push((lo, hi.min(first), p));
    }
    let (a, b) = (lo.max(first), hi.min(last));
    if a <= b {
        out.extend(knot_cuts(knots, p, n, a, b, continuity).ok()?);
    }
    if hi > last {
        out.push((lo.max(last), hi, n.checked_sub(1)?));
    }
    (out.len() <= CELLS).then_some(out)
}

/// The cuts of a box on a face's knots, `u` and `v` ([`cuts`]): `None` where they are
/// not enclosed or the box covers more than [`CELLS`] pieces of the face, so that one
/// enclosure does a fixed amount of work between the cancellation's polls.
fn cells(n: &Nurbs, u: Ivl, v: Ivl, continuity: usize, past_knots: bool) -> Option<CellCuts> {
    let us = cuts(&n.u, n.p, n.nu, (u.lo, u.hi), continuity, past_knots)?;
    let vs = cuts(&n.v, n.q, n.nv, (v.lo, v.hi), continuity, past_knots)?;
    (us.len().checked_mul(vs.len())? <= CELLS).then_some((us, vs))
}

/// A box's parts on a face's knots in `u` and in `v` ([`cells`]).
type CellCuts = (Vec<(f64, f64, usize)>, Vec<(f64, f64, usize)>);

/// The partials `P[a][b]`, `a + b <= lk`, of `S − o` over `u x v`, enclosed: for NURBS data
/// the hull over the pieces the box covers ([`nurbs_partials_over_piece_about`]: a rational
/// face's quotient then scales with the size of `S − o`, not with its distance from the
/// origin); for a definition its enclosure ([`definition_partials`]) less `o`, which is
/// not taken about `o`. `None` where a part is not enclosed.
fn partials_about(f: &Face<'_>, u: Ivl, v: Ivl, lk: usize, o: [f64; 3]) -> Option<Partials<Ivl>> {
    let (n, past_knots) = match f {
        Face::Nurbs { net, past_knots } => (net, *past_knots),
        Face::Definition { .. } => {
            let mut p = definition_partials(f, u, v, lk)?;
            let [x, y, z] = o.map(Ivl::from);
            p.set(0, 0, v_sub(p.at(0, 0), V3 { x, y, z }));
            return Some(p);
        }
    };
    let continuity = if lk >= 2 { CONTINUITY } else { 0 };
    let (us, vs) = cells(n, u, v, continuity, past_knots)?;
    let mut out: Option<Partials<Ivl>> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = nurbs_partials_over_piece_about(n, ku, kv, a0, a1, b0, b1, lk, o).ok()?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out
}

/// A definition's partials `P[a][b]`, `a + b <= lk`, over `u x v`, enclosed
/// ([`definition_partials_over_box`], its feet seeded on the net's piece at the box's
/// middle), or `None` (also for NURBS data).
fn definition_partials(f: &Face<'_>, u: Ivl, v: Ivl, lk: usize) -> Option<Partials<Ivl>> {
    let Face::Definition { kernel, net, def } = f else {
        return None;
    };
    let ku = span(&net.u, net.p, net.nu, u.mid());
    let kv = span(&net.v, net.q, net.nv, v.mid());
    let mut d = def.try_borrow_mut().ok()?;
    definition_partials_over_box(kernel, &mut d, net, ku, kv, u.lo, u.hi, v.lo, v.hi, lk).ok()
}

/// A face's partials `P[a][b]`, `a + b <= lk`, over `u x v`, enclosed: for NURBS data those
/// of `S − o` ([`partials_about`]) with `o` the face's point at the box's middle, `o` added
/// back to `P[0][0]`; for a definition its enclosure. `None` where a part is not enclosed.
fn partials_over(f: &Face<'_>, u: Ivl, v: Ivl, lk: usize) -> Option<Partials<Ivl>> {
    let (n, past_knots) = match f {
        Face::Nurbs { net, past_knots } => (net, *past_knots),
        Face::Definition { .. } => return definition_partials(f, u, v, lk),
    };
    let (um, vm) = (u.mid(), v.mid());
    let (us, vs) = cells(n, u, v, 0, past_knots)?;
    let (&(_, _, ku), &(_, _, kv)) = (
        us.iter().find(|c| c.0 <= um && um <= c.1)?,
        vs.iter().find(|c| c.0 <= vm && vm <= c.1)?,
    );
    let o = nurbs_partials_over_piece(n, ku, kv, um, um, vm, vm, 0)
        .ok()?
        .at(0, 0);
    let o = [o.x.mid(), o.y.mid(), o.z.mid()];
    let mut out = partials_about(f, u, v, lk, o)?;
    let [x, y, z] = o.map(Ivl::from);
    out.set(0, 0, v_add(out.at(0, 0), V3 { x, y, z }));
    Some(out)
}

/// `c x` for the rows `c`.
fn times(c: &[[f64; 3]; 3], x: [Ivl; 3]) -> [Ivl; 3] {
    c.map(|row| {
        row.iter()
            .zip(&x)
            .fold(ZERO, |s, (&r, &v)| s + Ivl::from(r) * v)
    })
}

fn neg(a: V3<Ivl>) -> V3<Ivl> {
    v_scale(a, Ivl::from(-1.0))
}

fn arr(a: V3<Ivl>) -> [Ivl; 3] {
    [a.x, a.y, a.z]
}

/// `|a|`, rounded up.
fn norm(a: V3<Ivl>) -> f64 {
    let sq = |x: Ivl| Ivl::from(x.mag()) * Ivl::from(x.mag());
    (sq(a.x) + sq(a.y) + sq(a.z)).sqrt().hi
}

/// The solutions `x` of `J x = b` for every matrix of the columns `cols` and every `b`,
/// enclosed: with `C` the inverse of `J`'s middle and `q = ||I − C J|| < 1`,
/// `|x| <= |C b| / (1 − q)` and `x ∈ C b + (I − C J) x`. `None` where `q >= 1`.
fn solve_over(cols: [V3<Ivl>; 3], b: V3<Ivl>) -> Option<[Ivl; 3]> {
    let mid = cols.map(|c| [c.x.mid(), c.y.mid(), c.z.mid()]);
    let c = inverse_of_columns3(&mid)?;
    let cols = cols.map(arr);
    let rows: [[Ivl; 3]; 3] =
        core::array::from_fn(|r| cols.map(|col| col.get(r).copied().unwrap_or(ZERO)));
    let (cm, q) = contraction(&c, &rows);
    (q < 1.0).then_some(())?;
    let cb = times(&c, arr(b));
    let size = cb.iter().fold(0.0_f64, |m, x| cmax(m, x.mag()));
    let r = (Ivl::from(size) / (Ivl::from(1.0) - Ivl::from(q))).hi;
    if !r.is_finite() {
        return None;
    }
    Some(plus_c_times(cb, &cm, &[Ivl::new(-r, r); 3]))
}

/// The rib's definition and its three curves, for the certificate of its spans.
pub(super) struct Rib<'a, 'f> {
    /// The fillet surface, enclosed.
    pub fillet: &'a Face<'f>,
    /// The kernel, polled for a cancellation.
    pub kernel: &'a Kernel,
    /// The end surface, enclosed.
    pub end: &'a Face<'f>,
    /// The fillet surface's section parameter: 0 u, 1 v.
    pub section: usize,
    /// `sigma(t) = sigma0 + d_sigma t^m`, `m` 2 for an odd contact order, else 1.
    pub sigma0: f64,
    /// See `sigma0`.
    pub d_sigma: f64,
    /// Whether the contact order is odd.
    pub odd: bool,
    /// The contact order.
    pub order: u32,
    /// The 3D curve, the fillet surface's UV curve and the end surface's, in `t`.
    pub curves: [NurbsCurve; 3],
    /// The end surface's standoff the rib is defined with ([`super::Standoff`]):
    /// `F(t, y) = S(sigma(t), alpha) + lambda(t) n − E(u, v)`, the rib's point the fillet
    /// surface's and its foot on the end surface `lambda(t) n` from it. `None`: `F = S − E`.
    pub standoff: Option<super::Standoff>,
}

/// A piece end: its parameter, the Newton root there, the box Krawczyk's test proves to
/// hold the one root there, and the errors' magnitudes over that box (3D x, y, z; fillet
/// sigma, alpha; end u, v).
#[derive(Clone, Copy)]
struct End {
    t: f64,
    y: [f64; 3],
    root: [Ivl; 3],
    e: [f64; 7],
}

/// The root's path over a piece ([`Rib::path_over`]): for every `t` of the piece the one
/// root in `c(t) + region` (the box Krawczyk's test ran on), `c` the quadratic about `tm`
/// with the coefficients `coef` (value, slope, half the second derivative, per unknown);
/// `hull` holds them all.
struct Path {
    tm: f64,
    coef: [(f64, f64, f64); 3],
    region: [Ivl; 3],
    hull: [Ivl; 3],
}

impl Path {
    /// Whether the box `root` at `t` lies inside `c(t) + region` for the exact `c(t)`: the
    /// one root it holds is then the path's, the only one there.
    fn holds(&self, t: f64, root: [Ivl; 3]) -> bool {
        let d = Ivl::from(t) - Ivl::from(self.tm);
        self.coef
            .iter()
            .zip(&self.region)
            .zip(&root)
            .all(|((&(m, be, ga), y), r)| {
                let c = Ivl::from(m) + Ivl::from(be) * d + Ivl::from(ga) * (d * d);
                // the box for the exact c(t) is at least [c.hi + y.lo, c.lo + y.hi]
                let (lo, hi) = (
                    (Ivl::from(c.hi) + Ivl::from(y.lo)).hi,
                    (Ivl::from(c.lo) + Ivl::from(y.hi)).lo,
                );
                lo <= r.lo && r.hi <= hi
            })
    }
}

/// `x^n` over `x`, by products (an enclosure for any sign of `x`).
fn power(x: Ivl, n: i32) -> Ivl {
    (0..n.max(0)).fold(Ivl::from(1.0), |p, _| p * x)
}

/// `a` scaled to the vector `n`.
fn along(n: [f64; 3], a: Ivl) -> V3<Ivl> {
    let [x, y, z] = n.map(|c| Ivl::from(c) * a);
    V3 { x, y, z }
}

impl Rib<'_, '_> {
    /// The standoff's `lambda(t) n` and its first two derivatives in `t` over `t`
    /// (`lambda = lambda0 (1 − t^m)`), or `None` without one.
    fn shift(&self, t: Ivl) -> Option<[V3<Ivl>; 3]> {
        let s = self.standoff?;
        let (m, l0) = (s.power, Ivl::from(s.lambda0));
        let n = [s.n.x, s.n.y, s.n.z];
        let f = |k: i32| Ivl::from(f64::from(k));
        let value = l0 * (Ivl::from(1.0) - power(t, m));
        let first = ZERO - l0 * f(m) * power(t, m - 1);
        let second = if m >= 2 {
            ZERO - l0 * f(m) * f(m - 1) * power(t, m - 2)
        } else {
            ZERO
        };
        Some([along(n, value), along(n, first), along(n, second)])
    }

    /// The standoff's `lambda` over the sections `sigma` (`lambda0 (1 − s^q)`, `s` the
    /// section parameter normalised, `q` the contact order), or `None` without one.
    fn standoff_over(&self, sigma: Ivl) -> Option<Ivl> {
        let s = self.standoff?;
        let q = if self.odd { s.power / 2 } else { s.power };
        let unit = (sigma - Ivl::from(self.sigma0)) / Ivl::from(self.d_sigma);
        Some(Ivl::from(s.lambda0) * (Ivl::from(1.0) - power(unit, q)))
    }

    fn sigma(&self, t: Ivl) -> (Ivl, Ivl, Ivl) {
        let d = Ivl::from(self.d_sigma);
        if self.odd {
            let two = Ivl::from(2.0);
            (Ivl::from(self.sigma0) + d * (t * t), two * d * t, two * d)
        } else {
            (Ivl::from(self.sigma0) + d * t, d, ZERO)
        }
    }

    /// The fillet surface's partials over `sigma x alpha`, and an accessor of
    /// `d^(i+j) S / dsigma^i dalpha^j`.
    fn fillet_over(
        &self,
        sigma: Ivl,
        alpha: Ivl,
        lk: usize,
    ) -> Option<impl Fn(usize, usize) -> V3<Ivl>> {
        self.fillet_about(sigma, alpha, lk, None)
    }

    /// [`Self::fillet_over`] of `S − o` (`None`: of `S`).
    fn fillet_about(
        &self,
        sigma: Ivl,
        alpha: Ivl,
        lk: usize,
        o: Option<[f64; 3]>,
    ) -> Option<impl Fn(usize, usize) -> V3<Ivl>> {
        let section = self.section;
        let (u, v) = if section == 0 {
            (sigma, alpha)
        } else {
            (alpha, sigma)
        };
        let p = match o {
            Some(o) => partials_about(self.fillet, u, v, lk, o)?,
            None => partials_over(self.fillet, u, v, lk)?,
        };
        Some(move |i, j| if section == 0 { p.at(i, j) } else { p.at(j, i) })
    }

    /// `F(t, y)`, enclosed: both faces less one point of the end surface near the root, so
    /// that `F` carries the rounding of their distance from it, not from the origin.
    fn value(&self, t: Ivl, y: [Ivl; 3]) -> Option<[Ivl; 3]> {
        let [a, u, v] = y;
        let o = partials_over(self.end, Ivl::from(u.mid()), Ivl::from(v.mid()), 0)?.at(0, 0);
        let o = [o.x.mid(), o.y.mid(), o.z.mid()];
        let s = self.fillet_about(self.sigma(t).0, a, 0, Some(o))?;
        let e = partials_about(self.end, u, v, 0, o)?;
        let f = v_sub(s(0, 0), e.at(0, 0));
        Some(arr(match self.shift(t) {
            Some([l, ..]) => v_add(f, l),
            None => f,
        }))
    }

    fn columns(&self, t: Ivl, y: [Ivl; 3]) -> Option<[[Ivl; 3]; 3]> {
        let [a, u, v] = y;
        let s = self.fillet_over(self.sigma(t).0, a, 1)?;
        let e = partials_over(self.end, u, v, 1)?;
        Some([arr(s(0, 1)), arr(neg(e.at(1, 0))), arr(neg(e.at(0, 1)))])
    }

    /// The box about `mid` that Krawczyk's test proves to hold the one root at `t`, or
    /// `None`.
    fn root_at(&self, t: f64, mid: [f64; 3]) -> Result<Option<[Ivl; 3]>> {
        let t = Ivl::from(t);
        let Some(j) = self.columns(t, mid.map(Ivl::from)) else {
            return Ok(None);
        };
        let Some(c) = inverse_of_columns3(&j.map(|col| col.map(Ivl::mid))) else {
            return Ok(None);
        };
        krawczyk(mid, c, &|y| Ok(self.value(t, y)), &|y| {
            Ok(self.columns(t, y))
        })
    }

    /// The box that holds the root for every `t` of the piece between `a` and `b` (the
    /// Newton root `mid` at its middle), or `None`. The root moves across the piece, so
    /// Krawczyk's test runs on `z = y − c(t)`, `c` the quadratic through the three Newton
    /// roots: `F(t, c(t))` is enclosed by its mean-value form about the middle, which
    /// shrinks with the piece's width squared where a fixed centre's shrinks with it once.
    /// `None` also where an end's box ([`End::root`]) is not inside the path there: its
    /// errors are then not proved to be the path's.
    fn path_over(&self, a: &End, b: &End, mid: [f64; 3]) -> Result<Option<Path>> {
        let tm = 0.5 * (a.t + b.t);
        let h = 0.5 * (b.t - a.t);
        // c(t) = mid + beta (t − t_m) + gamma (t − t_m)², per unknown
        let quad = |ya: f64, ym: f64, yb: f64| {
            (
                ym,
                (yb - ya) / (2.0 * h),
                (yb - 2.0 * ym + ya) / (2.0 * h * h),
            )
        };
        let ([a0, a1, a2], [m0, m1, m2], [b0, b1, b2]) = (a.y, mid, b.y);
        let coef = [quad(a0, m0, b0), quad(a1, m1, b1), quad(a2, m2, b2)];
        if !coef
            .iter()
            .all(|&(_, be, ga)| be.is_finite() && ga.is_finite())
        {
            return Ok(None);
        }
        let t = Ivl::new(a.t, b.t);
        let d = t - Ivl::from(tm);
        let two = Ivl::from(2.0);
        let c = coef.map(|(m, be, ga)| Ivl::from(m) + Ivl::from(be) * d + Ivl::from(ga) * (d * d));
        let [dc0, dc1, dc2] = coef.map(|(_, be, ga)| Ivl::from(be) + two * Ivl::from(ga) * d);
        let shift = |[z0, z1, z2]: [Ivl; 3]| {
            let [c0, c1, c2] = c;
            [c0 + z0, c1 + z1, c2 + z2]
        };
        let [ca, cu, cv] = c;
        // F(t, c(t)) ∈ F(t_m, mid) + (t − t_m) (S_sigma sigma' + F_y c') over the piece
        let value = |_: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
            let Some([x, y, z]) = self.value(Ivl::from(tm), mid.map(Ivl::from)) else {
                return Ok(None);
            };
            let (sigma, ds, _) = self.sigma(t);
            let Some(s) = self.fillet_over(sigma, ca, 1) else {
                return Ok(None);
            };
            let Some(e) = partials_over(self.end, cu, cv, 1) else {
                return Ok(None);
            };
            let slope = v_add(
                v_add(v_scale(s(1, 0), ds), v_scale(s(0, 1), dc0)),
                neg(v_add(v_scale(e.at(1, 0), dc1), v_scale(e.at(0, 1), dc2))),
            );
            // (with a standoff F's derivative in t gains lambda' n)
            let slope = match self.shift(t) {
                Some([_, l1, _]) => v_add(slope, l1),
                None => slope,
            };
            Ok(Some(arr(v_add(V3 { x, y, z }, v_scale(slope, d)))))
        };
        let Some(j) = self.columns(Ivl::from(tm), mid.map(Ivl::from)) else {
            return Ok(None);
        };
        let Some(inv) = inverse_of_columns3(&j.map(|col| col.map(Ivl::mid))) else {
            return Ok(None);
        };
        let Some((z, region)) =
            krawczyk_region([0.0; 3], inv, &value, &|z| Ok(self.columns(t, shift(z))))?
        else {
            return Ok(None);
        };
        let path = Path {
            tm,
            coef,
            region,
            hull: shift(z),
        };
        Ok((path.holds(a.t, a.root) && path.holds(b.t, b.root)).then_some(path))
    }

    /// The errors' magnitudes at `t`, the root in the box `y`.
    fn errors_at(&self, t: f64, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let at = Ivl::from(t);
        let [a, u, v] = y;
        let x = partials_over(self.end, u, v, 0)?.at(0, 0);
        // the rib's point: the end surface's, or with a standoff that point less lambda n
        let x = match self.shift(at) {
            Some([l, ..]) => v_sub(x, l),
            None => x,
        };
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, at, 0, None)
                .ok()
                .and_then(|d| d.first().copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x.x).mag(),
            (c3.y - x.y).mag(),
            (c3.z - x.z).mag(),
            (cs - self.sigma(at).0).mag(),
            (ca - a).mag(),
            (ce.x - u).mag(),
            (ce.y - v).mag(),
        ])
    }

    /// `|e''|` of the seven errors over the piece `t`, its root in the box `y`.
    fn second_over(&self, t: Ivl, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let [a, u, v] = y;
        let (_, ds, dds) = self.sigma(t);
        let s = self.fillet_over(self.sigma(t).0, a, 2)?;
        let e = partials_over(self.end, u, v, 2)?;
        let cols = [s(0, 1), neg(e.at(1, 0)), neg(e.at(0, 1))];
        let shift = self.shift(t);
        // J y' = −S_sigma sigma' (with a standoff, less lambda' n)
        let known = v_scale(s(1, 0), ds);
        let known = match shift {
            Some([_, l1, _]) => v_add(known, l1),
            None => known,
        };
        let [a1, u1, v1] = solve_over(cols, neg(known))?;
        let two = Ivl::from(2.0);
        let s2 = v_add(
            v_add(v_scale(s(2, 0), ds * ds), v_scale(s(1, 1), two * ds * a1)),
            v_add(v_scale(s(0, 2), a1 * a1), v_scale(s(1, 0), dds)),
        );
        // (with a standoff F's second derivative gains lambda'' n)
        let s2 = match shift {
            Some([_, _, l2]) => v_add(s2, l2),
            None => s2,
        };
        let e2 = v_add(
            v_add(
                v_scale(e.at(2, 0), u1 * u1),
                v_scale(e.at(1, 1), two * u1 * v1),
            ),
            v_scale(e.at(0, 2), v1 * v1),
        );
        // J y'' = −(the second derivative of F without its y'' terms)
        let [a2, u2, v2] = solve_over(cols, neg(v_sub(s2, e2)))?;
        let x2 = v_add(e2, v_add(v_scale(e.at(1, 0), u2), v_scale(e.at(0, 1), v2)));
        // the rib's point is the end surface's less lambda n
        let x2 = match shift {
            Some([_, _, l2]) => v_sub(x2, l2),
            None => x2,
        };
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, t, 2, None)
                .ok()
                .and_then(|d| d.get(2).copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x2.x).mag(),
            (c3.y - x2.y).mag(),
            (c3.z - x2.z).mag(),
            (cs - dds).mag(),
            (ca - a2).mag(),
            (ce.x - u2).mag(),
            (ce.y - v2).mag(),
        ])
    }

    /// `|e'|` of the seven errors over the piece `t`, its root in the box `y`.
    fn first_over(&self, t: Ivl, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let [a, u, v] = y;
        let (sigma, ds, _) = self.sigma(t);
        let s = self.fillet_over(sigma, a, 1)?;
        let e = partials_over(self.end, u, v, 1)?;
        let cols = [s(0, 1), neg(e.at(1, 0)), neg(e.at(0, 1))];
        let shift = self.shift(t);
        let known = v_scale(s(1, 0), ds);
        let known = match shift {
            Some([_, l1, _]) => v_add(known, l1),
            None => known,
        };
        let [a1, u1, v1] = solve_over(cols, neg(known))?;
        let x1 = v_add(v_scale(e.at(1, 0), u1), v_scale(e.at(0, 1), v1));
        let x1 = match shift {
            Some([_, l1, _]) => v_sub(x1, l1),
            None => x1,
        };
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, t, 1, None)
                .ok()
                .and_then(|d| d.get(1).copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x1.x).mag(),
            (c3.y - x1.y).mag(),
            (c3.z - x1.z).mag(),
            (cs - ds).mag(),
            (ca - a1).mag(),
            (ce.x - u1).mag(),
            (ce.y - v1).mag(),
        ])
    }

    /// The piece end at `t` from the Newton root `y` there: the root proved in a point box
    /// and the errors there, or `None`.
    fn end_at(&self, t: f64, y: [f64; 3]) -> Result<Option<End>> {
        let Some(root) = self.root_at(t, y)? else {
            return Ok(None);
        };
        Ok(self.errors_at(t, root).map(|e| End { t, y, root, e }))
    }

    /// The bound of the piece between `a` and `b` (the Newton root `mid` at its middle):
    /// the largest distance of the three curves from the rib over it, or `None`. Each
    /// error is bounded by the remainder about the piece's ends where `e''` encloses,
    /// else (or where that bound is over `tol`) by the centred form `|e(t_m)| + w/2 |e'|`
    /// about its middle, whichever is less.
    fn piece(&self, a: &End, b: &End, mid: [f64; 3], tol: f64) -> Result<Option<f64>> {
        let t = Ivl::new(a.t, b.t);
        let Some(path) = self.path_over(a, b, mid)? else {
            return Ok(None);
        };
        let y = path.hull;
        let w = Ivl::from(b.t) - Ivl::from(a.t);
        let mut c = [f64::INFINITY; 7];
        if let Some(e2) = self.second_over(t, y) {
            let rem = w * w / Ivl::from(8.0);
            for (((ci, &ea), &eb), &d2) in c.iter_mut().zip(&a.e).zip(&b.e).zip(&e2) {
                // (a NaN end error stays NaN, so `distance` refuses it)
                *ci =
                    (Ivl::from(if ea.is_nan() { ea } else { ea.max(eb) }) + rem * Ivl::from(d2)).hi;
            }
        }
        let first = self.distance(t, y, c);
        if first.is_some_and(|d| d <= tol) {
            return Ok(first);
        }
        // the middle's errors only where its box is inside the path there
        let Some(m) = self
            .end_at(0.5 * (a.t + b.t), mid)?
            .filter(|m| path.holds(m.t, m.root))
        else {
            return Ok(first);
        };
        let Some(e1) = self.first_over(t, y) else {
            return Ok(first);
        };
        let half = w / Ivl::from(2.0);
        for ((ci, &em), &d1) in c.iter_mut().zip(&m.e).zip(&e1) {
            *ci = ci.min((Ivl::from(em) + half * Ivl::from(d1)).hi);
        }
        Ok(self.distance(t, y, c))
    }

    /// The largest distance of the three curves from the rib over the piece `t` (its root
    /// in the box `y`) for the errors' bounds `c`, or `None`.
    fn distance(&self, t: Ivl, y: [Ivl; 3], c: [f64; 7]) -> Option<f64> {
        if !c.iter().all(|x| x.is_finite()) {
            return None;
        }
        let [cx, cy, cz, cs, ca, cu, cv] = c;
        let d3 = norm(V3 {
            x: Ivl::from(cx),
            y: Ivl::from(cy),
            z: Ivl::from(cz),
        });
        // a UV curve's point and the rib's lie in the box widened by the UV errors
        let wide = |x: Ivl, r: f64| x + Ivl::new(-r, r);
        let [ya, yu, yv] = y;
        let s = self.fillet_over(wide(self.sigma(t).0, cs), wide(ya, ca), 1)?;
        let e = partials_over(self.end, wide(yu, cu), wide(yv, cv), 1)?;
        let d_fil = (Ivl::from(norm(s(1, 0))) * Ivl::from(cs)
            + Ivl::from(norm(s(0, 1))) * Ivl::from(ca))
        .hi;
        let d_end = (Ivl::from(norm(e.at(1, 0))) * Ivl::from(cu)
            + Ivl::from(norm(e.at(0, 1))) * Ivl::from(cv))
        .hi;
        // every term finite (`f64::max` drops a NaN)
        let ds = [d3, d_fil, d_end];
        ds.iter()
            .all(|d| d.is_finite())
            .then(|| ds.into_iter().fold(0.0, f64::max))
    }

    /// The certified bound of the span `[t0, t1]` (the roots `y0`, `y1` at its stations)
    /// within `tol` (module docs): a piece not bounded within `tol` is halved, at most
    /// `LEVELS` times; [`Bound::Over`] or [`Bound::Unenclosed`] where one is not bounded
    /// within it after that. `solve` finds the roots inside it.
    pub(super) fn span_bound(
        &self,
        solve: &Solve<'_>,
        (t0, y0): (f64, [f64; 3]),
        (t1, y1): (f64, [f64; 3]),
        tol: f64,
    ) -> Result<Bound> {
        let (Some(a), Some(b)) = (self.end_at(t0, y0)?, self.end_at(t1, y1)?) else {
            return Ok(Bound::Unenclosed);
        };
        // a piece end not within tol of the rib by its enclosure: no finer cut bounds the
        // span (a cut moves no end)
        let off = |e: &End| {
            let [x, y, z, ..] = e.e;
            norm(V3 {
                x: Ivl::from(x),
                y: Ivl::from(y),
                z: Ivl::from(z),
            }) > tol
        };
        let mut worst = 0.0_f64;
        let mut pieces = vec![(a, b, 0)];
        while let Some((a, b, depth)) = pieces.pop() {
            self.kernel.check_cancelled(OP)?;
            if off(&a) || off(&b) {
                return Ok(Bound::Over);
            }
            let mut guess = a.y;
            for (g, &bi) in guess.iter_mut().zip(&b.y) {
                *g = 0.5 * (*g + bi);
            }
            let t = 0.5 * (a.t + b.t);
            let mid = solve(t, guess)?.unwrap_or(guess);
            match self.piece(&a, &b, mid, tol)? {
                Some(d) if d <= tol => worst = worst.max(d),
                _ if depth < LEVELS => {
                    let Some(m) = self.end_at(t, mid)? else {
                        return Ok(Bound::Unenclosed);
                    };
                    pieces.push((m, b, depth + 1));
                    pieces.push((a, m, depth + 1));
                }
                Some(_) => return Ok(Bound::Over),
                None => return Ok(Bound::Unenclosed),
            }
        }
        Ok(Bound::Within(worst))
    }

    /// The certificate of the first span `[0, t1]` (the root `y1` at `t1`, `p` at the
    /// tangent end) (#17744). Its pieces are graded geometrically toward the tangent end,
    /// `[t/2, t]`, each bounded as a span ([`Self::span_bound`]): the Jacobian is singular
    /// at the tangent end, and over `[t/2, t]` its least singular value changes by the
    /// same factor at every depth, so a piece needs as many halvings at every depth. The
    /// pieces go on while they are certified, down to `t*`, the lower end of the last one;
    /// `[0, t*]` is bounded without the Jacobian ([`Self::near_bound`]). Where that bound
    /// is not within the tolerance, the span is [`Bound::Over`] if the piece that stopped
    /// the grading was over it, [`Bound::Unenclosed`] if no piece was certified (`t* =
    /// t1`; more stations shorten the span), else [`Bound::Open`]: `[0, t*]` carries no
    /// claim.
    pub(super) fn first_span_bound(
        &self,
        solve: &Solve<'_>,
        p: [f64; 3],
        (t1, y1): (f64, [f64; 3]),
        tol: f64,
    ) -> Result<Bound> {
        let m = self.growth();
        let mut worst = 0.0_f64;
        let (mut hi_t, mut hi_y) = (t1, y1);
        let mut stop = Bound::Open;
        for _ in 0..GRADES {
            self.kernel.check_cancelled(OP)?;
            let lo_t = 0.5 * hi_t;
            let mut guess = p;
            for (g, &h) in guess.iter_mut().zip(&hi_y) {
                *g += (h - *g) * 0.5_f64.powi(i32::try_from(m).unwrap_or(1));
            }
            let Some(lo_y) = solve(lo_t, guess)? else {
                break;
            };
            match self.span_bound(solve, (lo_t, lo_y), (hi_t, hi_y), tol)? {
                Bound::Within(b) => {
                    worst = worst.max(b);
                    hi_t = lo_t;
                    hi_y = lo_y;
                }
                other => {
                    stop = other;
                    break;
                }
            }
        }
        if let Some(near) = self.near_bound(p, (hi_t, hi_y), tol)? {
            if near <= tol {
                return Ok(Bound::Within(worst.max(near)));
            }
        }
        Ok(match stop {
            Bound::Over => Bound::Over,
            _ if hi_t >= t1 => Bound::Unenclosed,
            _ => Bound::Open,
        })
    }

    /// The tangent end's frame: the fillet surface's point `S(sigma0, alpha0)` and the end
    /// surface's unit normal at `(u0, v0)` (the fillet surface's too, at a tangent end; the
    /// end surface's keeps the separation's system regular wherever its own partials are),
    /// or `None`.
    fn frame(&self, [a0, u0, v0]: [f64; 3]) -> Option<Frame> {
        let at = Ivl::from;
        let s = self.fillet_over(at(self.sigma0), at(a0), 0)?;
        let e = partials_over(self.end, at(u0), at(v0), 1)?;
        let m = |a: V3<Ivl>| [a.x.mid(), a.y.mid(), a.z.mid()];
        let (p, [ax, ay, az], [bx, by, bz]) = (m(s(0, 0)), m(e.at(1, 0)), m(e.at(0, 1)));
        let n = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
        let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        let n = n.map(|x| x / len);
        // (a standoff is along its own copy of this normal: the separation is taken along
        // the same one, so that the rib's is lambda exactly)
        let n = self.standoff.map_or(n, |s| [s.n.x, s.n.y, s.n.z]);
        n.iter().all(|x| x.is_finite()).then_some(Frame { p, n })
    }

    /// The separation's system `G = (S(sigma, alpha) − p) + lambda n − (E(u, v) − p)` in
    /// `y = (lambda, u, v)` at a point, as numbers, for [`newton`].
    fn separation_at(&self, f: &Frame, sigma: f64, alpha: f64, y: [f64; 3]) -> Linearised<3> {
        let m = |a: V3<Ivl>| [a.x.mid(), a.y.mid(), a.z.mid()];
        let s = self.fillet_about(Ivl::from(sigma), Ivl::from(alpha), 0, Some(f.p))?;
        let e = partials_about(self.end, Ivl::from(y[1]), Ivl::from(y[2]), 1, f.p)?;
        let ([sx, sy, sz], [ex, ey, ez]) = (m(s(0, 0)), m(e.at(0, 0)));
        let [nx, ny, nz] = f.n;
        let g = [
            sx + y[0] * nx - ex,
            sy + y[0] * ny - ey,
            sz + y[0] * nz - ez,
        ];
        Some((g, [f.n, m(neg(e.at(1, 0))), m(neg(e.at(0, 1)))]))
    }

    /// The box of the separation `y = (lambda, u, v)`, `S(sigma, alpha) + lambda n =
    /// E(u, v)`, that holds its one root for every `(sigma, alpha)` of the box, or `None`:
    /// Krawczyk's test about the root at the box's middle (Newton's from `start`), `G` at it
    /// enclosed by its mean-value form, `G(mid) + S_sigma (sigma − sigma_m) + S_alpha
    /// (alpha − alpha_m)`. `G(mid)` is taken less `p`, so that it carries the rounding of
    /// the faces' distances from the tangent end, not from the origin (generic-blend.md
    /// §2.4, as the nets are). The test runs on `C G`, `C` the inverse of `G`'s partials at
    /// the middle; a box it proves holds `C G = 0`, and `C` is then regular (Krawczyk's
    /// strict inclusion proves `C` and every `G′` over the box regular), so `G = 0`.
    fn separation_over(
        &self,
        f: &Frame,
        sigma: Ivl,
        alpha: Ivl,
        start: [f64; 3],
    ) -> Result<Option<[Ivl; 3]>> {
        let (sm, am) = (sigma.mid(), alpha.mid());
        let Some(mid) = newton(
            start,
            &|y| Ok(self.separation_at(f, sm, am, y)),
            &inverse_of_columns3,
        )?
        else {
            return Ok(None);
        };
        let n = V3 {
            x: Ivl::from(f.n[0]),
            y: Ivl::from(f.n[1]),
            z: Ivl::from(f.n[2]),
        };
        let raw = |y: [Ivl; 3]| {
            partials_over(self.end, y[1], y[2], 1)
                .map(|e| [arr(n), arr(neg(e.at(1, 0))), arr(neg(e.at(0, 1)))])
        };
        let Some(j) = raw(mid.map(Ivl::from)) else {
            return Ok(None);
        };
        let Some(c) = inverse_of_columns3(&j.map(|col| col.map(Ivl::mid))) else {
            return Ok(None);
        };
        // the system preconditioned, `C G`: `C S_sigma` and `C S_alpha` are formed before
        // they are scaled, so that lambda's row keeps their cancellation (its slope is
        // `Δκ a`, not `|S_alpha|`)
        let value = |_: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
            let at = Ivl::from;
            let (Some(s0), Some(s), Some(e0)) = (
                self.fillet_about(at(sm), at(am), 0, Some(f.p)),
                self.fillet_over(sigma, alpha, 1),
                partials_about(self.end, at(mid[1]), at(mid[2]), 0, f.p),
            ) else {
                return Ok(None);
            };
            let g = v_add(v_sub(s0(0, 0), e0.at(0, 0)), v_scale(n, at(mid[0])));
            let [g, ds, da] = [g, s(1, 0), s(0, 1)].map(|x| times(&c, arr(x)));
            let (hs, ha) = (sigma - at(sm), alpha - at(am));
            Ok(Some(core::array::from_fn(|i| {
                let pick = |x: [Ivl; 3]| x.get(i).copied().unwrap_or(ZERO);
                pick(g) + pick(ds) * hs + pick(da) * ha
            })))
        };
        let columns = |y: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
            Ok(raw(y).map(|cols| cols.map(|col| times(&c, col))))
        };
        let one = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        krawczyk(mid, one, &value, &columns)
    }

    /// Whether the separation keeps one sign over the boxes `place(x, b)` for every `x` in
    /// `x0` and `b` in `b0` (`place` gives the box's sections, its alphas and how far its
    /// centre moves across `x`, or `None`): the box, cut across its longer side on the
    /// faces while the separation's enclosure holds 0, at most [`RING_BOXES`] boxes.
    fn one_sign(
        &self,
        f: &Frame,
        (x0, b0): (Ivl, Ivl),
        place: &Place<'_>,
        start: [f64; 3],
    ) -> Result<bool> {
        let Some(s) = place(x0, b0).and_then(|(s, a, _)| self.fillet_over(s, a, 1)) else {
            return Ok(false);
        };
        let (ls, la) = (norm(s(1, 0)), norm(s(0, 1)));
        let mut boxes = vec![(x0, b0)];
        let mut seen = 0;
        while let Some((x, b)) = boxes.pop() {
            self.kernel.check_cancelled(OP)?;
            seen += 1;
            if seen > RING_BOXES {
                return Ok(false);
            }
            let Some((s, a, moved)) = place(x, b) else {
                return Ok(false);
            };
            if let Some([lambda, ..]) = self.separation_over(f, s, a, start)? {
                // the rib's separation is 0, or with a standoff lambda at its section
                let lambda = match self.standoff_over(s) {
                    Some(held) => lambda - held,
                    None => lambda,
                };
                if lambda.lo > 0.0 || lambda.hi < 0.0 {
                    continue;
                }
            }
            let (xm, bm) = (x.mid(), b.mid());
            let moved = if moved > 0.0 { moved * la } else { 0.0 };
            if (s.hi - s.lo) * ls + moved >= (b.hi - b.lo) * la {
                if !(x.lo < xm && xm < x.hi) {
                    return Ok(false);
                }
                boxes.push((Ivl::new(x.lo, xm), b));
                boxes.push((Ivl::new(xm, x.hi), b));
            } else {
                if !(b.lo < bm && bm < b.hi) {
                    return Ok(false);
                }
                boxes.push((x, Ivl::new(b.lo, bm)));
                boxes.push((x, Ivl::new(bm, b.hi)));
            }
        }
        Ok(true)
    }

    /// The power `m` the offsets from the tangent end grow as, `t^m`: alpha ~ s^(k/2),
    /// s ~ t or t^2.
    fn growth(&self) -> u32 {
        if self.odd { self.order } else { self.order / 2 }.max(1)
    }

    /// The fillet UV curve's alpha `alpha_c` and its slope over `t`, or `None`.
    fn centre_over(&self, t: Ivl) -> Option<(Ivl, Ivl)> {
        let d = curve_derivs_over(self.curves.get(1)?, t, 1, None).ok()?;
        let (&v, &s) = (d.first()?, d.get(1)?);
        Some(if self.section == 0 {
            (v.y, s.y)
        } else {
            (v.x, s.x)
        })
    }

    /// The certified bound of the rib over `[0, t*]` (the root `ys` at `t*`, `p` at the
    /// tangent end), or `None` (#17744, the maintainer's design on #18674). The Jacobian
    /// is singular at the tangent end, so the root is not enclosed there; instead:
    ///
    /// - **The rib stays near the tangent end.** With `A` twice the root's offset in alpha
    ///   at `t*` (Krawczyk's box there), the separation `lambda` of the end surface from
    ///   the section `S(sigma, alpha)` along the tangent end's normal ([`Self::frame`],
    ///   [`Self::separation_over`]) keeps one sign over `sigma(0..t*)` and `A <= |alpha −
    ///   alpha0| <= 2A` ([`Self::one_sign`]). The rib's root is continuous in `t`, lies
    ///   within `A/2` at `t*` and has `lambda = 0`, so it never crosses that ring: `|alpha −
    ///   alpha0| < A` over `[0, t*]`. ARGUED, not checked: on the ring the rib's end UV is
    ///   the separation's root in its box (the one root there), that is the end surface
    ///   meets the section only once near the tangent end (it is one-to-one there), and the
    ///   rib is one continuous branch from the tangent end to `t*` (the ring bounds a branch
    ///   that exists; it does not prove that one does). Where the ring holds a sign change, `A` is doubled
    ///   (the gap `½ Δκ (A² − a(t*)²) − θ A` grows with it), at most [`WIDENINGS`] times,
    ///   while `|S_alpha| A` and the bound below are within `tol` ([`Self::fixed_ring`]).
    /// - **Bands about the fillet UV curve** (#19140), where that ring costs more than `tol`:
    ///   it costs at least about `2 |S_alpha| a(t*)`, `a(t*)` the root's offset from `alpha0`,
    ///   since its centre stays at `alpha0` while the root moves away from it. The same test
    ///   runs about a moving centre, the fillet UV curve's own `alpha_c(t)`, on pieces graded
    ///   toward the tangent end, and the ring about `alpha0` then covers only the part below
    ///   them, where `a` is smaller ([`Self::moving_bands`]). Where `Δκ` vanishes neither
    ///   clears, and `[0, t*]` is not bounded ([`Bound::Open`]).
    /// - **The curves against known functions** ([`Self::near_errors`]): each curve's 3D
    ///   image against `S(sigma(t), alpha0)` (in a band `S(sigma(t), alpha_c(t))`) at the
    ///   same `t`, plus `|S_alpha|` times the ring's inner radius. No unknown is enclosed and
    ///   no Jacobian is inverted there.
    fn near_bound(&self, p: [f64; 3], (ts, ys): (f64, [f64; 3]), tol: f64) -> Result<Option<f64>> {
        let [a0, u0, v0] = p;
        let start = [0.0, u0, v0];
        let Some(f) = self.frame(p) else {
            return Ok(None);
        };
        let Some(b) = self.root_at(ts, ys)? else {
            return Ok(None);
        };
        let offset = (b[0] - Ivl::from(a0)).mag();
        let ring = self.fixed_ring(&f, (a0, start), ts, (2.0 * offset, WIDENINGS), tol)?;
        Ok(match ring {
            Ring::Within(d) => Some(d),
            // the ring about alpha0 costs more than tol: the bands about alpha_c
            Ring::Over => self.moving_bands(&f, (a0, start), ts, b[0], tol)?,
            Ring::Open => None,
        })
    }

    /// The ring `A <= |alpha − alpha0| <= 2A` over `[0, t_hi]` ([`Self::near_bound`]), from
    /// `big_a`, the root at `t_hi` less than `big_a` from `alpha0`, doubled at most
    /// `widenings − 1` times.
    fn fixed_ring(
        &self,
        f: &Frame,
        (a0, start): (f64, [f64; 3]),
        t_hi: f64,
        (big_a, widenings): (f64, usize),
        tol: f64,
    ) -> Result<Ring> {
        let t = Ivl::new(0.0, t_hi);
        let sigma = self.sigma(t).0;
        let at = Ivl::from;
        let place = |s: Ivl, a: Ivl| Some((s, a, 0.0));
        let mut big_a = big_a;
        for _ in 0..widenings {
            self.kernel.check_cancelled(OP)?;
            let alpha = at(a0) + Ivl::new(-big_a, big_a);
            let Some(s) = self.fillet_over(sigma, alpha, 1) else {
                return Ok(Ring::Open);
            };
            if !big_a.is_finite() || big_a <= 0.0 {
                return Ok(Ring::Open);
            }
            // past the tolerance the bound cannot clear it, and a wider ring only adds to it
            if norm(s(0, 1)) * big_a > tol {
                return Ok(Ring::Over);
            }
            let band = Band {
                t,
                dt: t,
                t_ref: 0.0,
                sigma_ref: at(self.sigma0),
                centre_ref: at(a0),
                centre: at(a0),
                slope: None,
                alpha,
                half: big_a,
            };
            let Some(d) = self.near_errors(&band).filter(|d| !d.is_nan()) else {
                return Ok(Ring::Open);
            };
            if d > tol {
                return Ok(Ring::Over);
            }
            let ring = |lo: f64, hi: f64| at(a0) + Ivl::new(lo, hi);
            if self.one_sign(f, (sigma, ring(big_a, 2.0 * big_a)), &place, start)?
                && self.one_sign(f, (sigma, ring(-2.0 * big_a, -big_a)), &place, start)?
            {
                return Ok(Ring::Within(d));
            }
            big_a *= 2.0;
        }
        Ok(Ring::Open)
    }

    /// The bands of [`Self::near_bound`] (#19140, after the maintainer's sketch on #18674),
    /// the root's alpha `root` at `t*`: the bound within `tol`, or `None`. Band `k` is
    /// `T_k = [q t_k, t_k]`, `t_0 = t*`, `q` near enough to 1 that the root's offset `a`
    /// falls by at most half across it, with the inner radius `h_k`, two thirds of the
    /// centre's distance from `alpha0` at `q t_k` (the root's mirror, the other root of
    /// the section and E, lies about `2 |alpha_c − alpha0|` off), at least `h_(k−1) / 2`.
    /// Where the separation keeps one sign on `h_k <= |alpha − alpha_c(t)| <= 2 h_k` over
    /// `T_k` ([`Self::one_sign`], cut on `t` and on the offset, the centre enclosed over each
    /// piece), the root stays within `h_k` of the centre over `T_k`: at `t_k` it is within
    /// `h_(k−1) <= 2 h_k` (at `t*` within `|root − alpha_c(t*)|`, which must be at most `2 h_0`)
    /// and off the ring, and it is continuous (ARGUED as the ring's branch above). Below
    /// `T_k` the ring about `alpha0` takes over, the root at `q t_k` within
    /// `|alpha_c − alpha0| + h_k` of `alpha0` ([`Self::fixed_ring`], one ring, not widened);
    /// where that ring too costs more than `tol`, the next band. A band is tried only while
    /// its own bound, about `|S_alpha| h_k`, is within `tol` and the ring below it is not, so
    /// few bands run.
    fn moving_bands(
        &self,
        f: &Frame,
        (a0, start): (f64, [f64; 3]),
        ts: f64,
        root: Ivl,
        tol: f64,
    ) -> Result<Option<f64>> {
        let at = Ivl::from;
        let Some((mut c_hi, _)) = self.centre_over(at(ts)) else {
            return Ok(None);
        };
        let place = |x: Ivl, b: Ivl| {
            let (c, _) = self.centre_over(x)?;
            Some((self.sigma(x).0, c + b, c.hi - c.lo))
        };
        // a band's lower end: `t q` with `q = 2^(−1/2^j)`, `2^j >= m`, so that the offset
        // `a ~ t^m` falls by at most half across it (`h_k >= h_(k−1) / 2` then needs no
        // floor); square roots only, rounded the same everywhere
        let (m, mut q, mut p) = (self.growth(), 0.5_f64, 1_u32);
        while p < m {
            q = sqrt(q);
            p = p.saturating_mul(2);
        }
        let (mut worst, mut hi, mut half) = (0.0_f64, ts, 0.0_f64);
        let mut inside = (root - c_hi).mag();
        for _ in 0..GRADES {
            self.kernel.check_cancelled(OP)?;
            let lo = q * hi;
            let t = Ivl::new(lo, hi);
            let (Some((c, slope)), Some((c_lo, _))) =
                (self.centre_over(t), self.centre_over(at(lo)))
            else {
                return Ok(None);
            };
            // the inner ring's separation at its edges, `−h` and `−2h` from the centre, is
            // `½ Δκ h (2a − h)` and `2 Δκ h (a − h)` with `a = |alpha_c − alpha0|`: equal
            // for `h = 2a / 3`, both `4/9 Δκ a²` (the outer ring's least is twice that); `a`
            // at the band's lower end, nearest the tangent end
            let h = 2.0 / 3.0 * (c_lo - at(a0)).mig();
            // (a NaN fails it)
            if !(h > 0.0 && (2.0 * h).is_finite()) {
                return Ok(None);
            }
            half = h.max(0.5 * half);
            if inside.is_nan() || inside > 2.0 * half {
                return Ok(None);
            }
            let band = Band {
                t,
                dt: t - at(hi),
                t_ref: hi,
                sigma_ref: self.sigma(at(hi)).0,
                centre_ref: c_hi,
                centre: c,
                slope: Some(slope),
                alpha: c + Ivl::new(-half, half),
                half,
            };
            let Some(d) = self.near_errors(&band).filter(|&d| d <= tol) else {
                return Ok(None);
            };
            let ring = |lo: f64, hi: f64| (t, Ivl::new(lo, hi));
            if !(self.one_sign(f, ring(half, 2.0 * half), &place, start)?
                && self.one_sign(f, ring(-2.0 * half, -half), &place, start)?)
            {
                return Ok(None);
            }
            worst = worst.max(d);
            // the ring starts at the root's farthest alpha there (it lies strictly inside),
            // and widens
            let offset = (at((c_lo - at(a0)).mag()) + at(half)).hi;
            match self.fixed_ring(f, (a0, start), lo, (offset, 1), tol)? {
                Ring::Within(d) => return Ok(Some(worst.max(d))),
                Ring::Over => {}
                Ring::Open => return Ok(None),
            }
            (hi, c_hi, inside) = (lo, c_lo, half);
        }
        Ok(None)
    }

    /// The bound of [`Self::near_bound`] over a band `b.t` about a centre, the root's alpha
    /// within `b.half` of it: each curve's 3D image `X(t)` (the 3D curve; a UV curve's point
    /// on its surface) against the rib's point `S(sigma(t), alpha(t))`, which lies in
    /// `S(sigma(t), c(t)) + S_alpha [−h, h]`, `c` the centre (`alpha0`, or `alpha_c` in a
    /// band). `X(t) − S(sigma(t), c(t))` is bounded by its mean-value form from the band's
    /// reference `t_r` (0, or the band's upper end), `X(t_r) − S(sigma(t_r), c(t_r)) + (t −
    /// t_r) (X′ − S_sigma sigma′ − S_alpha c′)`, with `X′` enclosed over the curve's box on its
    /// surface (a face's knot line there is one it is continuous across, [`partials_over`],
    /// so `X` is continuous and smooth piecewise).
    fn near_errors(&self, b: &Band) -> Option<f64> {
        let at = Ivl::from;
        let t = b.t;
        let (sigma, ds, _) = self.sigma(t);
        let s = self.fillet_over(sigma, b.alpha, 1)?;
        let s_0 = self.fillet_over(sigma, b.centre, 1)?;
        let s_p = self.fillet_over(b.sigma_ref, b.centre_ref, 0)?;
        let off = v_scale(s(0, 1), Ivl::new(-b.half, b.half));
        let known = v_scale(s_0(1, 0), ds);
        let known = match b.slope {
            Some(c1) => v_add(known, v_scale(s_0(0, 1), c1)),
            None => known,
        };
        let pick = |c: V3<Ivl>| {
            if self.section == 0 {
                (c.x, c.y)
            } else {
                (c.y, c.x)
            }
        };
        // each curve's image at the reference t and its derivative over t
        let mut images = Vec::with_capacity(3);
        for (i, c) in self.curves.iter().enumerate() {
            let c0 = *curve_derivs_over(c, at(b.t_ref), 0, None).ok()?.first()?;
            let ct = curve_derivs_over(c, t, 1, None).ok()?;
            let (&range, &d1) = (ct.first()?, ct.get(1)?);
            images.push(match i {
                0 => (c0, d1),
                1 => {
                    let ((fs, fa), (rs, ra), (ds1, da1)) = (pick(c0), pick(range), pick(d1));
                    let f0 = self.fillet_over(fs, fa, 0)?;
                    let f = self.fillet_over(rs, ra, 1)?;
                    (
                        f0(0, 0),
                        v_add(v_scale(f(1, 0), ds1), v_scale(f(0, 1), da1)),
                    )
                }
                _ => {
                    let e0 = partials_over(self.end, c0.x, c0.y, 0)?;
                    let e = partials_over(self.end, range.x, range.y, 1)?;
                    let d = v_add(v_scale(e.at(1, 0), d1.x), v_scale(e.at(0, 1), d1.y));
                    // with a standoff the image is the rib's foot on the end surface: less
                    // lambda n it is the rib's point
                    match (self.shift(at(b.t_ref)), self.shift(t)) {
                        (Some([l, ..]), Some([_, l1, _])) => (v_sub(e0.at(0, 0), l), v_sub(d, l1)),
                        _ => (e0.at(0, 0), d),
                    }
                }
            });
        }
        let mut worst = 0.0_f64;
        for (x0, x1) in images {
            let e = v_add(
                v_add(v_sub(x0, s_p(0, 0)), v_scale(v_sub(x1, known), b.dt)),
                off,
            );
            let d = norm(e);
            // (`f64::max` drops a NaN)
            if !d.is_finite() {
                return None;
            }
            worst = worst.max(d);
        }
        worst.is_finite().then_some(worst)
    }
}

/// What the ring about `alpha0` says of `[0, t_hi]` ([`Rib::fixed_ring`]).
enum Ring {
    /// Bounded within the tolerance.
    Within(f64),
    /// Its bound is over the tolerance at the least ring it may take, and only grows with
    /// a wider one.
    Over,
    /// Not bounded otherwise: a sign change on every ring tried, or a part not enclosed.
    Open,
}

/// The tangent end's point and unit normal, as numbers ([`Rib::frame`]).
struct Frame {
    p: [f64; 3],
    n: [f64; 3],
}

/// The sections and alphas of a ring's box from its parameters `(x, b)`, with how far its
/// centre moves across `x` ([`Rib::one_sign`]), or `None`.
type Place<'a> = dyn Fn(Ivl, Ivl) -> Option<(Ivl, Ivl, f64)> + 'a;

/// A part of the rib near the tangent end about a centre ([`Rib::near_errors`]).
struct Band {
    /// The curves' parameters.
    t: Ivl,
    /// `t` less the reference parameter `t_ref`.
    dt: Ivl,
    /// The parameter the mean-value form is taken from, with the section and the centre
    /// there.
    t_ref: f64,
    sigma_ref: Ivl,
    centre_ref: Ivl,
    /// The centre over `t`, and its slope in `t` (`None`: a fixed centre).
    centre: Ivl,
    slope: Option<Ivl>,
    /// The root's alphas over `t`, the centre widened by `half`.
    alpha: Ivl,
    half: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Vec2;
    use crate::geom::vector3d::{Point3, Vec3};
    use crate::surface::cone::Cone;
    use crate::surface::plane::Plane;

    /// A face is enclosed where the kernel evaluates its net (review on #18674): a
    /// cylinder takes its point from its STEP form, so it is not enclosed; a plane clamps
    /// its parameters to its domain, its net's knots, so a box inside them is enclosed and
    /// one past them is not.
    #[test]
    fn a_face_is_enclosed_only_where_the_kernel_evaluates_its_net() {
        let k = Kernel::default();
        let cylinder = Surface::from(Cone::new_cylinder_bounded(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::X,
            Vec3::Y,
            2.0,
            0.0,
            360.0,
            3.0,
            false,
            false,
            false,
        ));
        assert!(Face::of(&k, &cylinder).unwrap().is_none());
        let plane = Surface::from(Plane::new(
            Point3::new(0.0, 0.0, 1.0),
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ));
        let face = Face::of(&k, &plane).unwrap().unwrap();
        let inside = Ivl::new(0.25, 0.5);
        assert!(partials_over(&face, inside, inside, 1).is_some());
        assert!(partials_over(&face, Ivl::new(0.75, 1.0 + 1e-9), inside, 1).is_none());
        assert!(partials_over(&face, inside, Ivl::new(-1e-9, 0.25), 1).is_none());
    }

    /// A piece end's errors count only where its box lies inside the path there, `c(t) +
    /// Y` for the exact `c(t)`, `Y` the box the path's test ran on (review on #18674):
    /// the path `c(t) = 1 + 2 (t − 1/2)` per unknown with `Y` within 1e-9 is about 0.5 at
    /// `t = 1/4`.
    #[test]
    fn an_end_box_counts_only_inside_the_path() {
        let path = Path {
            tm: 0.5,
            coef: [(1.0, 2.0, 0.0); 3],
            region: [Ivl::new(-1e-9, 1e-9); 3],
            hull: [Ivl::new(0.0, 3.0); 3],
        };
        let around = |c: f64, r: f64| [Ivl::new(c - r, c + r); 3];
        assert!(path.holds(0.25, around(0.5, 1e-12)));
        assert!(!path.holds(0.25, around(0.5 + 2e-9, 1e-12)));
        assert!(!path.holds(0.25, around(0.5, 2e-9)));
    }
}
