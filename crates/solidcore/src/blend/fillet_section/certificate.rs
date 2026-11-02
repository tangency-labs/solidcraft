//! The G^n section's certificate: a proved bound on how far a Hermite span between two
//! stations is from the section of the true jets, from rounded interval arithmetic (the
//! feet over the span enclosed by Krawczyk's operator, their Taylor coefficients order by
//! order, and the Hermite remainder `h^4 / 384 max |c''''|`).
//!
//! For the rational section the blend's distance from its definition (the section of the
//! true jets `c_ik(s)` at every s) is at most `max_i sum_k |c_ik - C_ik|`, `C_ik` the jets'
//! Hermite curves: f is in `[0, 1]` and `|u - 1| <= 1` in `T_i`. On a span of length h a
//! cubic Hermite interpolant of a function's values and derivatives at both ends is off it
//! by at most `h^4 / 384` times the largest fourth derivative. That bound comes from
//! rounded interval arithmetic: the feet over the span are enclosed by Krawczyk's operator,
//! which proves a unique root of G in its box for every s in the span when it maps the box
//! into its interior and the preconditioned Jacobian's norm is below one; the feet's Taylor
//! coefficients, to order 4 in s and n in tau, are then enclosed order by order through the
//! same preconditioned system.
//!
//! Fork: `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib@11ea2d9), the
//! certificate (lines 825-1430); not in the pinned upstream.
//!
//! | Rust                              | Fork (`SmFilletSection.cpp`)            |
//! |-----------------------------------|-----------------------------------------|
//! | [`NurbsCurve`], [`NurbsCurve::of`] | `sm_NurbsCurve`, `sm_NurbsCurveOf`     |
//! | [`curve_derivs_over`]             | `sm_CurveDerivsOver`                    |
//! | [`nurbs_partials_over_piece`]     | `sm_NurbsPartialsOverPiece`             |
//! | [`leader_series_over`]            | `sm_LeaderSeriesOver`                   |
//! | [`GnLaw`]                         | `sm_GnLaw` (0d43b4c)                    |
//! | [`law_series`]                    | `sm_LawSeries` (0d43b4c)                |
//! | [`law_series_over`]               | `sm_LawSeriesOver` (0d43b4c)            |
//! | [`inverse4`]                      | `sm_Inverse4`                           |
//! | [`Enclosure`], [`gn_enclose`]     | `sm_GnEnclose` (its `rF`, `pX`, `pDS`)  |
//! | [`SpanAt`], [`gn_enclose_over`], [`face_partials_over`] | `sm_GnSpanAt`, `sm_GnEncloseOver`, `sm_FacePartialsOver` (c5b6bb7) |
//! | [`gn_fourth_over`]                | `sm_GnFourthOver`                       |
//! | [`gn_certify_span`]               | `sm_GnCertifySpan`                      |
//! | `gn_try!` (private)               | `SM_GN_TRY`                             |
//!
//! Deviations:
//! - **Out-params.** Every function returns its results. `sm_GnEnclose`'s `rbProved` FALSE
//!   is `Ok(None)`; its optional `pX` and `pDS` are always returned (computing them changes
//!   nothing else). `sm_CurveDerivsOver`'s `lPiece = -1` is `None`.
//!   `sm_NurbsPartialsOverPiece` returns fresh partials instead of overwriting the low
//!   orders of the caller's (the orders it leaves are never read).
//! - **`SM_GN_TRY`.** A failed evaluation inside the certificate proves nothing, as in the
//!   fork: `gn_enclose` returns `Ok(None)`, `gn_fourth_over` its unproved (infinite)
//!   bounds, `gn_certify_span` the bound it has so far. Only a truncation past the series'
//!   fixed size (`n > SM_GN_MAXE`, which the fork leaves to its callers) is an `Err`.
//! - **Bounds.** Every loop is the fork's, with its fixed counts (8 Krawczyk steps, 200
//!   contraction steps, levels 0..=6); there is no recursion.
//! - **Indices.** Knots and nets are read by checked indices (a knot or a coordinate
//!   outside reads NaN, which shows in the result); the fork's indices stay inside.

use super::definition_box::{definition_partials_over_box, GnDefFace, GnDefs};
use super::form::{binom, gn_form_normalized, GnSectionFormT};
use super::interval::{cmax, cmin, round_down, round_up, Ivl};
use super::series::{GnScalar, Ser, Trunc, VSer, MAX_E, MAX_S, TAYLOR, V3};
use super::station::{
    ders_basis, gn_evaluate, gn_evaluate_vn, gn_jacobian, gn_solve_station_counted, gn_top_jets,
    knot, smax, solve4, span, GnStation, GnSystem, Nurbs, Partials,
};
use super::tay::{
    gn_area_sum, gn_half_angle, gn_invert, gn_target, tay_sqrt, GnAreaCells, Ring, Tay, TayScalar,
    TAY,
};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;

const OP: &str = "blend::fillet_section::certificate";

/// `SM_GN_TRY(a)`: a failed evaluation proves nothing, the enclosing function returns
/// `Ok($none)` (the fork's `return SM_SUCCESS` with its outputs as they stand).
macro_rules! gn_try {
    ($e:expr, $none:expr) => {
        match $e {
            Ok(v) => v,
            Err(_) => return Ok($none),
        }
    };
}

/// `sm_NurbsCurve`: a NURBS curve's data (the leader), as [`Nurbs`].
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    /// The degree.
    pub p: usize,
    /// The number of control points.
    pub n: usize,
    /// The full knot vector.
    pub u: Vec<f64>,
    /// The homogeneous control points `(x w, y w, z w, w)`.
    pub pw: Vec<f64>,
}

impl NurbsCurve {
    /// `sm_NurbsCurveOf`: the data of a B-spline curve.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the curve has no NURB, its degree, knots and control points do not
    /// agree, or a weight is not positive.
    pub fn of(c: &BSplineCurve) -> Result<Self> {
        let (knots, mult) = c.knots(None)?;
        let (cps, ws) = c.control_polygon()?;
        let p = c.degree()?;
        let n = cps.len();
        let mut u = Vec::new();
        for (&k, &m) in knots.iter().zip(&mult) {
            for _ in 0..m {
                u.push(k);
            }
        }
        if p < 1 || u.len() != n + p + 1 {
            // "SmGnSectionFSG: the centre line's NURBS data do not agree"
            return Err(SolidError::sm_err(OP));
        }
        let rational = ws.len() == cps.len();
        let mut pw = Vec::new();
        for (i, cp) in cps.iter().enumerate() {
            let w = if rational {
                ws.get(i).copied().unwrap_or(f64::NAN)
            } else {
                1.0
            };
            let positive = w > 0.0;
            if !positive {
                return Err(SolidError::sm_err(OP));
            }
            pw.extend_from_slice(&[cp.x * w, cp.y * w, cp.z * w, w]);
        }
        Ok(Self { p, n, u, pw })
    }
}

/// `crPw[i]` (NaN outside).
#[inline]
fn pw_at(pw: &[f64], i: usize) -> f64 {
    pw.get(i).copied().unwrap_or(f64::NAN)
}

/// `rM[i][j]` of a nested vector (0 outside; the fork's indices stay inside).
#[inline]
fn at2<T: Copy>(m: &[Vec<T>], i: usize, j: usize, zero: T) -> T {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(zero)
}

/// `v[i]` of a vector (0 outside).
#[inline]
fn at1<T: Copy>(v: &[T], i: usize, zero: T) -> T {
    v.get(i).copied().unwrap_or(zero)
}

const IZ: Ivl = Ivl::new(0.0, 0.0);
const VZ: V3<Ivl> = V3 {
    x: IZ,
    y: IZ,
    z: IZ,
};

/// `sm_CurveDerivsOver`: the derivatives `D[0..=ld]` of the curve's polynomial piece
/// `piece` (the span of the knots it starts at; `None`: each piece `s` covers) for every
/// parameter in `s`, enclosed (hulled over the pieces).
///
/// # Errors
///
/// `SM_ERR` when no piece covers `s`.
pub fn curve_derivs_over(
    cn: &NurbsCurve,
    s: Ivl,
    ld: usize,
    piece: Option<usize>,
) -> Result<Vec<V3<Ivl>>> {
    let k0 = piece.unwrap_or_else(|| span(&cn.u, cn.p, cn.n, s.lo));
    let k1 = piece.unwrap_or_else(|| span(&cn.u, cn.p, cn.n, s.hi));
    let fixed = piece.is_some();
    let p = cn.p;
    let mut first = true;
    let mut rd = vec![VZ; ld + 1];
    for k in k0..=k1 {
        let ki = k as isize;
        let has_span = knot(&cn.u, ki) < knot(&cn.u, ki + 1);
        if !has_span {
            continue; // a knot repeated: no span
        }
        let a = if k == p || fixed {
            s.lo
        } else {
            cmax(s.lo, knot(&cn.u, ki))
        };
        let b = if k + 1 == cn.n || fixed {
            s.hi
        } else {
            cmin(s.hi, knot(&cn.u, ki + 1))
        };
        // an interval that ends on a knot is the span it lies in, not also the next one's
        // end point
        if a > b || (a == b && s.lo < s.hi) {
            continue;
        }
        // the homogeneous curve is a polynomial of degree p on the span: its derivatives
        // over [a, b] are its Taylor polynomials at the middle m, from exact point
        // derivatives (rounding only)
        let m = 0.5 * (a + b);
        let nb = ders_basis(&cn.u, k, p, Ivl::from(m), p);
        let mut am = vec![VZ; p + 1];
        let mut wm = vec![IZ; p + 1];
        for d in 0..=p {
            let (mut x, mut y, mut z, mut w) = (IZ, IZ, IZ, IZ);
            for j in 0..=p {
                // k >= p (sm_Span)
                let li = 4 * (k + j - p);
                let nj = at2(&nb, d, j, IZ);
                x = x + nj * Ivl::from(pw_at(&cn.pw, li));
                y = y + nj * Ivl::from(pw_at(&cn.pw, li + 1));
                z = z + nj * Ivl::from(pw_at(&cn.pw, li + 2));
                w = w + nj * Ivl::from(pw_at(&cn.pw, li + 3));
            }
            if let (Some(ad), Some(wd)) = (am.get_mut(d), wm.get_mut(d)) {
                *ad = V3 { x, y, z };
                *wd = w;
            }
        }
        let dt = Ivl::new(a, b) - Ivl::from(m);
        let mut av = vec![VZ; ld + 1];
        let mut wv = vec![IZ; ld + 1];
        for d in 0..=ld {
            let (mut x, mut y, mut z, mut w, mut pwr) = (IZ, IZ, IZ, IZ, Ivl::from(1.0));
            let mut f = 1.0_f64;
            for j in d..=p {
                if j > d {
                    pwr = pwr * dt;
                    f *= (j - d) as f64;
                }
                let c = pwr * Ivl::from(1.0 / f);
                let aj = at1(&am, j, VZ);
                x = x + aj.x * c;
                y = y + aj.y * c;
                z = z + aj.z * c;
                w = w + at1(&wm, j, IZ) * c;
            }
            if let (Some(ad), Some(wd)) = (av.get_mut(d), wv.get_mut(d)) {
                *ad = V3 { x, y, z };
                *wd = w;
            }
        }
        let w0 = at1(&wv, 0, IZ);
        let mut cv = vec![VZ; ld + 1];
        for d in 0..=ld {
            let mut v = at1(&av, d, VZ);
            let mut c = 1.0_f64;
            for i in 1..=d {
                c = c * (d - i + 1) as f64 / i as f64;
                let wi = at1(&wv, i, IZ);
                let cdi = at1(&cv, d - i, VZ);
                v.x = v.x - Ivl::from(c) * wi * cdi.x;
                v.y = v.y - Ivl::from(c) * wi * cdi.y;
                v.z = v.z - Ivl::from(c) * wi * cdi.z;
            }
            if let Some(cd) = cv.get_mut(d) {
                *cd = V3 {
                    x: v.x / w0,
                    y: v.y / w0,
                    z: v.z / w0,
                };
            }
        }
        for (r, c) in rd.iter_mut().zip(&cv) {
            if first {
                *r = *c;
            } else {
                r.x = r.x.hull(c.x);
                r.y = r.y.hull(c.y);
                r.z = r.z.hull(c.z);
            }
        }
        first = false;
    }
    if first {
        return Err(SolidError::sm_err(OP));
    }
    Ok(rd)
}

/// `sm_NurbsPartialsOverPiece`: the partials `P[a][b]`, `a + b <= lk`, of one polynomial
/// piece (spans `ku`, `kv`) over `[ua, ub] x [va, vb]`, enclosed: the homogeneous surface's
/// Taylor polynomials at the box's middle, from exact point derivatives (it is of degree
/// p, q there), then the rational quotient's (the NURBS book's A4.4) in intervals.
///
/// # Errors
///
/// `SM_ERR` where the weights are not proved positive over the box ("a face's weights are
/// not positive over a box").
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_piece(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    let um = 0.5 * (ua + ub);
    let vm = 0.5 * (va + vb);
    let nu = ders_basis(&cn.u, ku, cn.p, Ivl::from(um), cn.p);
    let nv = ders_basis(&cn.v, kv, cn.q, Ivl::from(vm), cn.q);
    let (la, lb) = (cn.p, cn.q);
    // homogeneous point derivatives Hm[a][b] (x, y, z, w) at the middle
    let mut hm = vec![vec![[IZ; 4]; lb + 1]; la + 1];
    for (a, row) in hm.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            let mut h = [IZ; 4];
            for i in 0..=cn.p {
                for j in 0..=cn.q {
                    // ku >= p and kv >= q (sm_Span)
                    let li = 4 * ((ku + i - cn.p) * cn.nv + (kv + j - cn.q));
                    let c = at2(&nu, a, i, IZ) * at2(&nv, b, j, IZ);
                    for (e, he) in h.iter_mut().enumerate() {
                        *he = *he + c * Ivl::from(pw_at(&cn.pw, li + e));
                    }
                }
            }
            *hab = h;
        }
    }
    let du = Ivl::new(ua, ub) - Ivl::from(um);
    let dv = Ivl::new(va, vb) - Ivl::from(vm);
    let mut pu = vec![IZ; la + 1];
    let mut pv = vec![IZ; lb + 1];
    if let (Some(u0), Some(v0)) = (pu.first_mut(), pv.first_mut()) {
        *u0 = Ivl::from(1.0);
        *v0 = Ivl::from(1.0);
    }
    for i in 1..=la {
        let x = at1(&pu, i - 1, IZ) * du * Ivl::from(1.0 / i as f64);
        if let Some(pi) = pu.get_mut(i) {
            *pi = x;
        }
    }
    for j in 1..=lb {
        let x = at1(&pv, j - 1, IZ) * dv * Ivl::from(1.0 / j as f64);
        if let Some(pj) = pv.get_mut(j) {
            *pj = x;
        }
    }
    let mut av = vec![vec![VZ; lk + 1]; lk + 1];
    let mut wv = vec![vec![IZ; lk + 1]; lk + 1];
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut h = [IZ; 4];
            for i in a..=la {
                for j in b..=lb {
                    let c = at1(&pu, i - a, IZ) * at1(&pv, j - b, IZ);
                    let hij = at2(&hm, i, j, [IZ; 4]);
                    for (he, &x) in h.iter_mut().zip(&hij) {
                        *he = *he + x * c;
                    }
                }
            }
            if let (Some(ab), Some(wab)) = (
                av.get_mut(a).and_then(|r| r.get_mut(b)),
                wv.get_mut(a).and_then(|r| r.get_mut(b)),
            ) {
                let [x, y, z, w] = h;
                *ab = V3 { x, y, z };
                *wab = w;
            }
        }
    }
    let w00 = at2(&wv, 0, 0, IZ);
    let positive = w00.lo > 0.0;
    if !positive {
        // "SmGnSectionFSG: a face's weights are not positive over a box"
        return Err(SolidError::sm_err(OP));
    }
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut v3 = at2(&av, a, b, VZ);
            let mut cka = 1.0_f64;
            for i in 0..=a {
                if i > 0 {
                    cka = cka * (a - i + 1) as f64 / i as f64;
                }
                let mut cbj = 1.0_f64;
                for j in 0..=b {
                    if j > 0 {
                        cbj = cbj * (b - j + 1) as f64 / j as f64;
                    }
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let c = Ivl::from(cka * cbj) * at2(&wv, i, j, IZ);
                    let pij = out.at(a - i, b - j);
                    v3.x = v3.x - c * pij.x;
                    v3.y = v3.y - c * pij.y;
                    v3.z = v3.z - c * pij.z;
                }
            }
            out.set(
                a,
                b,
                V3 {
                    x: v3.x / w00,
                    y: v3.y / w00,
                    z: v3.z / w00,
                },
            );
        }
    }
    Ok(out)
}

/// `sm_LeaderSeriesOver`: the leader's point and unit tangent over `s` as series in sigma
/// to `tr.s`, enclosed (the span lies in one piece of the leader: the stations hold its
/// knots). `piece`: the leader's piece whose polynomial is taken over `s` (`lPiece`,
/// c5b6bb7; `None`, the fork's -1: the one the middle of `s` lies in).
///
/// # Errors
///
/// Those of [`curve_derivs_over`]; `SM_ERR` where the tangent is not proved non-zero.
pub fn leader_series_over(
    leader: &NurbsCurve,
    s: Ivl,
    tr: Trunc,
    piece: Option<usize>,
) -> Result<(VSer<Ivl>, VSer<Ivl>)> {
    let sigma = Trunc::new(tr.s(), 0)?;
    let piece = piece.unwrap_or_else(|| span(&leader.u, leader.p, leader.n, s.mid()));
    let d = curve_derivs_over(leader, s, tr.s() + 1, Some(piece))?;
    let mut l = VSer::constant(VZ);
    let mut d1 = l;
    let mut fact = 1.0_f64;
    for j in 0..=tr.s() {
        if j > 0 {
            fact *= j as f64;
        }
        let f = Ivl::from(1.0 / fact);
        let dj = at1(&d, j, VZ);
        let dj1 = at1(&d, j + 1, VZ);
        let cl = [dj.x, dj.y, dj.z];
        let cd = [dj1.x, dj1.y, dj1.z];
        for ((la, da), (&x, &y)) in l.a.iter_mut().zip(d1.a.iter_mut()).zip(cl.iter().zip(&cd)) {
            la.set(j, 0, x * f);
            da.set(j, 0, y * f);
        }
    }
    let inv = d1.dot(&d1, sigma).inv_sqrt(sigma)?;
    let t = d1.times(&inv, sigma);
    Ok((l, t))
}

/// `sm_GnLaw` (0d43b4c): a ball whose radius varies, its law r(s) over the leader's
/// parameter (the x of a B-spline curve), with the curve's NURBS data.
#[derive(Clone, Debug)]
pub struct GnLaw<'a> {
    /// `pCurve`.
    pub curve: &'a BSplineCurve,
    /// `sN`.
    pub n: NurbsCurve,
}

impl<'a> GnLaw<'a> {
    /// The law of `curve` (`sLaw.pCurve = pLaw; sm_NurbsCurveOf(*pLaw, sLaw.sN)`).
    ///
    /// # Errors
    ///
    /// Those of [`NurbsCurve::of`].
    pub fn of(curve: &'a BSplineCurve) -> Result<Self> {
        Ok(Self {
            curve,
            n: NurbsCurve::of(curve)?,
        })
    }
}

/// `sm_LawSeries` (0d43b4c): r at `s` to order `order` in sigma (its derivatives past a
/// polynomial law's degree zero), `None` without a law; `before`: the law's piece before
/// `s` at one of its knots.
///
/// # Errors
///
/// Those of the law's evaluation; `SM_ERR` where the radius is not positive ("the radius
/// law is not positive").
pub fn law_series(
    law: Option<&GnLaw<'_>>,
    s: f64,
    order: usize,
    before: bool,
) -> Result<Option<Ser<f64>>> {
    let Some(law) = law else {
        return Ok(None);
    };
    let mut r = Ser::constant(0.0_f64);
    let mut derivs = order;
    if !law.curve.is_rational()? {
        derivs = derivs.min(law.curve.degree()?);
    }
    if derivs > MAX_S {
        return Err(SolidError::sm_err(OP));
    }
    let d = law.curve.evaluate(s, derivs, !before, false)?;
    let mut fact = 1.0_f64;
    for i in 0..=derivs {
        if i > 0 {
            fact *= i as f64;
        }
        let di = d.get(i).ok_or_else(|| SolidError::sm_err(OP))?;
        r.set(i, 0, di.x / fact);
    }
    let positive = r.at(0, 0) > 0.0;
    if !positive {
        // "SmGnSectionFSG: the radius law is not positive"
        return Err(SolidError::sm_err(OP));
    }
    Ok(Some(r))
}

/// `sm_LawSeriesOver` (0d43b4c): r over the span `s` (in one piece of the law: the
/// stations hold its knots) to order `order`, enclosed.
///
/// # Errors
///
/// Those of [`curve_derivs_over`]; `SM_ERR` where r is not proved positive.
pub fn law_series_over(law: &GnLaw<'_>, s: Ivl, order: usize) -> Result<Ser<Ivl>> {
    let mut r = Ser::constant(IZ);
    let piece = span(&law.n.u, law.n.p, law.n.n, s.mid());
    let d = curve_derivs_over(&law.n, s, order, Some(piece))?;
    let mut fact = 1.0_f64;
    for i in 0..=order {
        if i > 0 {
            fact *= i as f64;
        }
        r.set(i, 0, at1(&d, i, VZ).x * Ivl::from(1.0 / fact));
    }
    if !r.at(0, 0).is_positive() {
        return Err(SolidError::sm_err(OP));
    }
    Ok(r)
}

/// `sm_Inverse4`: the inverse of a 4 x 4 matrix (the preconditioner; any approximation
/// will do), column by column with [`solve4`].
///
/// # Errors
///
/// Those of [`solve4`] (a singular matrix).
pub fn inverse4(a: &[[f64; 4]; 4]) -> Result<[[f64; 4]; 4]> {
    let mut y = [[0.0_f64; 4]; 4];
    for j in 0..4 {
        let mut e = [0.0_f64; 4];
        if let Some(ej) = e.get_mut(j) {
            *ej = 1.0;
        }
        let col = solve4(a, &e)?;
        for (row, &c) in y.iter_mut().zip(&col) {
            if let Some(x) = row.get_mut(j) {
                *x = c;
            }
        }
    }
    Ok(y)
}

/// The proved enclosure of a span (`sm_GnEnclose`'s outputs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enclosure {
    /// The feet's series over the span (`rF`).
    pub f: [VSer<Ivl>; 2],
    /// The feet's parameters' series (`pX`).
    pub x: [Ser<Ivl>; 4],
    /// Per face, `|S_u| + |S_v|` over the feet's box (`pDS`): it bounds `|dS|` per unit uv
    /// step (max norm).
    pub ds: [f64; 2],
    /// The same, least (`pDSLo`): `|S_u| + |S_v|` no smaller anywhere in the box.
    pub ds_lo: [f64; 2],
    /// The point V's series over the span (`pV`, 7102bc5).
    pub v: VSer<Ivl>,
}

fn ivl_at(x: &[Ivl; 4], i: usize) -> Ivl {
    x.get(i).copied().unwrap_or(IZ)
}

fn f_at(x: &[f64; 4], i: usize) -> f64 {
    x.get(i).copied().unwrap_or(f64::NAN)
}

/// The system over the partials `p` (`sSys` with `apP` at `asP`).
fn system<'a>(
    p: &'a [Partials<Ivl>; 2],
    leader: &VSer<Ivl>,
    tangent: &VSer<Ivl>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&'a Ser<Ivl>>,
) -> GnSystem<'a, Ivl> {
    let [p0, p1] = p;
    GnSystem {
        p: [p0, p1],
        leader: *leader,
        tangent: *tangent,
        r,
        side,
        hold: false,
        law,
    }
}

/// A face's partials over `[ua, ub] x [va, vb]`, enclosed (`sm_FacePartialsOver`,
/// c5b6bb7): a NURBS face's piece `(ku, kv)`, continued; a blend by its definition's on
/// the piece of its net along s that `(ku, kv)` is in
/// ([`super::definition_box::definition_partials_over_box`]).
///
/// # Errors
///
/// Those of [`nurbs_partials_over_piece`] and
/// [`super::definition_box::definition_partials_over_box`].
#[allow(clippy::too_many_arguments)]
pub fn face_partials_over(
    k: &Kernel,
    cn: &Nurbs,
    def: Option<&mut GnDefFace<'_>>,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    if let Some(d) = def {
        return definition_partials_over_box(k, d, cn, ku, kv, ua, ub, va, vb, lk);
    }
    nurbs_partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk)
}

/// A span the feet are enclosed over (`sm_GnSpanAt`, c5b6bb7): S, the feet's guesses at
/// its ends (their hull, widened, is the first box), each face's polynomial piece over it
/// and the leader's (`None`: the one the middle of S lies in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanAt {
    /// `sS`.
    pub s: Ivl,
    /// `axA`: the feet's guesses at S's start.
    pub xa: [f64; 4],
    /// `axB`: and at its end.
    pub xb: [f64; 4],
    /// `aku`: each face's piece in u.
    pub ku: [usize; 2],
    /// `akv`: and in v.
    pub kv: [usize; 2],
    /// `lLeader`.
    pub leader: Option<usize>,
}

/// `sm_GnEnclose`: the feet's series over `S = [a.s, b.s]` (a point when they are one
/// station), to order `order_s` in s and `n` in tau, enclosed; `None` where Krawczyk's test
/// fails on S (or an evaluation fails: nothing is proved). `defs`: the faces that are
/// blends by their definitions, enclosed over the boxes (c5b6bb7).
///
/// # Errors
///
/// `InvalidInput` when `order_s` or `n` exceeds the series' size.
#[allow(clippy::too_many_arguments)]
pub fn gn_enclose(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<Option<Enclosure>> {
    let ax = |c: usize| a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
    let bx = |c: usize| b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
    let mut at = SpanAt {
        s: Ivl::new(a.s, b.s),
        xa: [ax(0), ax(1), ax(2), ax(3)],
        xb: [bx(0), bx(1), bx(2), bx(3)],
        ku: [0; 2],
        kv: [0; 2],
        leader: None,
    };
    // each face's polynomial piece over the span: the one its middle lies in. A station
    // stands where a foot crosses a knot line, so every foot over the span lies in that
    // piece, and the piece's polynomial, continued over the rest of a box, is the face
    // wherever a foot can be: the root Krawczyk's test proves unique in a box is the foot
    // (a definition's piece of its net is one between two of its stations, its polynomial
    // pieces the ones the faces under it have there)
    for (f, face) in faces_n.iter().enumerate() {
        let su = span(&face.u, face.p, face.nu, 0.5 * (ax(2 * f) + bx(2 * f)));
        let sv = span(
            &face.v,
            face.q,
            face.nv,
            0.5 * (ax(2 * f + 1) + bx(2 * f + 1)),
        );
        if let (Some(u), Some(v)) = (at.ku.get_mut(f), at.kv.get_mut(f)) {
            *u = su;
            *v = sv;
        }
    }
    gn_enclose_over(k, faces_n, leader_n, &at, r, side, n, order_s, law, defs)
}

/// `sm_GnEncloseOver` (c5b6bb7): the feet's series over `at.s`, to order `order_s` in s and
/// `n` in tau, enclosed; `None` where Krawczyk's test fails on S (or an evaluation fails:
/// nothing is proved). The centre's residual is evaluated once: the iterations keep the
/// centre.
///
/// # Errors
///
/// `InvalidInput` when `order_s` or `n` exceeds the series' size.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn gn_enclose_over(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    at: &SpanAt,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<Option<Enclosure>> {
    let s = at.s;
    let tr = Trunc::new(order_s, n)?;
    // a ball whose radius varies: its law over the span (0d43b4c)
    let law_r = match law {
        Some(l) => Some(gn_try!(law_series_over(l, s, order_s), None)),
        None => None,
    };
    let law_r = law_r.as_ref();
    let (leader, tangent) = gn_try!(leader_series_over(leader_n, s, tr, at.leader), None);
    let ax = |c: usize| at.xa.get(c).copied().unwrap_or(f64::NAN);
    let bx = |c: usize| at.xb.get(c).copied().unwrap_or(f64::NAN);
    let [ku0, ku1] = at.ku;
    let [kv0, kv1] = at.kv;
    let mut defs = defs;
    let mut partials_over = |xl: [f64; 4], xh: [f64; 4], lk: usize| -> Result<[Partials<Ivl>; 2]> {
        let [n0, n1] = faces_n;
        let [l0, l1, l2, l3] = xl;
        let [h0, h1, h2, h3] = xh;
        let (d0, d1) = match defs.as_deref_mut() {
            Some(d) => {
                let [a0, a1] = &mut d.faces;
                (a0.as_mut(), a1.as_mut())
            }
            None => (None, None),
        };
        Ok([
            face_partials_over(k, n0, d0, ku0, kv0, l0, h0, l1, h1, lk)?,
            face_partials_over(k, n1, d1, ku1, kv1, l2, h2, l3, h3, lk)?,
        ])
    };

    // the feet's box: Krawczyk's operator from the guesses' hull
    let mut xm = [0.0_f64; 4];
    let mut bx_ = [IZ; 4];
    for (c, (m, xc)) in xm.iter_mut().zip(bx_.iter_mut()).enumerate() {
        let (lo, hi) = (ax(c), bx(c));
        *m = 0.5 * (lo + hi);
        let w = 0.5 * (hi - lo).abs() + 1.0e-9 * (1.0 + m.abs());
        *xc = Ivl::new(*m - 2.0 * w, *m + 2.0 * w);
    }
    let mut y = [[0.0_f64; 4]; 4];
    let mut cm = [[IZ; 4]; 4];
    let mut q = f64::INFINITY;
    let mut proved = false;
    let point = Trunc::new(0, 0)?;
    // G at the box's centre (which the iterations keep) for every s in the span
    let sg = {
        let pc = gn_try!(partials_over(xm, xm, 2), None);
        let sxc = xm.map(|x| Ser::constant(Ivl::from(x)));
        let (sg, _) = gn_try!(
            gn_evaluate(&system(&pc, &leader, &tangent, r, side, law_r), &sxc, point),
            None
        );
        sg
    };
    let mut it = 0;
    while it < 8 && !proved {
        it += 1;
        // the Jacobian over the box
        let pb = gn_try!(partials_over(bx_.map(|x| x.lo), bx_.map(|x| x.hi), 2), None);
        let sxb = bx_.map(Ser::constant);
        let jac = gn_try!(
            gn_jacobian(&system(&pb, &leader, &tangent, r, side, law_r), &sxb),
            None
        );
        let jm = jac.map(|row| row.map(Ivl::mid));
        y = gn_try!(inverse4(&jm), None);
        q = 0.0;
        for (i, (yi, ci)) in y.iter().zip(cm.iter_mut()).enumerate() {
            let mut row = 0.0_f64;
            for (j, cij) in ci.iter_mut().enumerate() {
                let mut c = Ivl::from(if i == j { 1.0 } else { 0.0 });
                for (&yik, jk) in yi.iter().zip(&jac) {
                    c = c - Ivl::from(yik) * jk.get(j).copied().unwrap_or(IZ);
                }
                *cij = c;
                row = round_up(row + c.mag());
            }
            q = cmax(q, row);
        }
        let mut kk = [IZ; 4];
        let mut inside = q < 1.0;
        for (i, ((yi, ci), ki)) in y.iter().zip(&cm).zip(kk.iter_mut()).enumerate() {
            let mut k = Ivl::from(f_at(&xm, i));
            for (&yij, gj) in yi.iter().zip(&sg) {
                k = k - Ivl::from(yij) * gj.at(0, 0);
            }
            for (j, &cij) in ci.iter().enumerate() {
                k = k + cij * (ivl_at(&bx_, j) - Ivl::from(f_at(&xm, j)));
            }
            *ki = k;
            let xi = ivl_at(&bx_, i);
            if !(k.lo > xi.lo && k.hi < xi.hi) {
                inside = false;
            }
        }
        if inside {
            for (xi, ki) in bx_.iter_mut().zip(&kk) {
                *xi = Ivl::new(cmax(xi.lo, ki.lo), cmin(xi.hi, ki.hi));
            }
            proved = true;
        } else {
            let contracts = q < 1.0;
            if !contracts {
                return Ok(None); // the span is too long for its Jacobian to be one preconditioner's
            }
            // the next box from the contraction: K's radii are at most d + |C| w for a box
            // of radii w about the centre, d those of Y G, so the box of radii
            // w = (I - |C|)^-1 d holds its K; a half more, to clear rounding and the change
            // of C with the box
            let mut d = [0.0_f64; 4];
            for (di, yi) in d.iter_mut().zip(&y) {
                let mut v = IZ;
                for (&yij, gj) in yi.iter().zip(&sg) {
                    v = v + Ivl::from(yij) * gj.at(0, 0);
                }
                *di = (v - Ivl::from(v.mid())).mag() + v.mid().abs();
            }
            let mut w = d;
            for _ in 0..200 {
                let mut wn = [0.0_f64; 4];
                for ((wni, &di), ci) in wn.iter_mut().zip(&d).zip(&cm) {
                    *wni = di;
                    for (cij, &wj) in ci.iter().zip(&w) {
                        *wni += cij.mag() * wj;
                    }
                }
                w = wn;
            }
            for ((xi, &wi), &mi) in bx_.iter_mut().zip(&w).zip(&xm) {
                let rr = 1.5 * wi + 1.0e-12 * (1.0 + mi.abs());
                if !rr.is_finite() {
                    return Ok(None);
                }
                *xi = Ivl::new(mi - rr, mi + rr);
            }
        }
    }
    if !proved {
        return Ok(None);
    }

    // the feet's Taylor coefficients over the span, order by order: x = -Y R + C x,
    // |x| <= rho
    let lk = tr.s() + tr.e() + 1;
    let pk = gn_try!(
        partials_over(bx_.map(|x| x.lo), bx_.map(|x| x.hi), lk),
        None
    );
    let sysk = system(&pk, &leader, &tangent, r, side, law_r);
    let mut sx = bx_.map(Ser::constant);
    for tot in 1..=(tr.s() + tr.e()) {
        let (sg, _) = gn_try!(gn_evaluate(&sysk, &sx, tr), None);
        for i in 0..=cmin_usize(tr.s(), tot) {
            let k = tot - i;
            // the section's jets of order n are the face curve's (gn_top_jets, 3bc54a4)
            if k > tr.e() || (n >= 1 && k >= n) {
                continue;
            }
            let mut yr = [IZ; 4];
            let mut rho = 0.0_f64;
            for (yrr, yrow) in yr.iter_mut().zip(&y) {
                let mut v = IZ;
                for (&yrc, gc) in yrow.iter().zip(&sg) {
                    v = v - Ivl::from(yrc) * gc.at(i, k);
                }
                *yrr = v;
                rho = cmax(rho, v.mag());
            }
            rho = round_up(rho / (1.0 - q));
            if !rho.is_finite() {
                return Ok(None);
            }
            for ((sxr, &yrr), crow) in sx.iter_mut().zip(&yr).zip(&cm) {
                let mut v = yrr;
                for &crc in crow {
                    v = v + crc * Ivl::new(-rho, rho);
                }
                sxr.set(i, k, v);
            }
        }
    }
    let (_, mut f, v, nrm) = gn_try!(gn_evaluate_vn(&sysk, &sx, tr), None);
    gn_top_jets(n, tr, &nrm, &mut f);
    // the faces' first partials over the box: |S_u| + |S_v| bounds |dS| per unit uv step
    // (max norm)
    let mut ds = [0.0_f64; 2];
    let mut ds_lo = [0.0_f64; 2];
    for ((dsf, dsl), pf) in ds.iter_mut().zip(ds_lo.iter_mut()).zip(&pk) {
        let su = pf.at(1, 0);
        let sv = pf.at(0, 1);
        let du = crate::geom::vector3d::sqrt(
            su.x.mag() * su.x.mag() + su.y.mag() * su.y.mag() + su.z.mag() * su.z.mag(),
        );
        let dv = crate::geom::vector3d::sqrt(
            sv.x.mag() * sv.x.mag() + sv.y.mag() * sv.y.mag() + sv.z.mag() * sv.z.mag(),
        );
        *dsf = du + dv;
        let lu = crate::geom::vector3d::sqrt(
            su.x.mig() * su.x.mig() + su.y.mig() * su.y.mig() + su.z.mig() * su.z.mig(),
        );
        let lv = crate::geom::vector3d::sqrt(
            sv.x.mig() * sv.x.mig() + sv.y.mig() * sv.y.mig() + sv.z.mig() * sv.z.mig(),
        );
        *dsl = lu + lv;
    }
    Ok(Some(Enclosure {
        f,
        x: sx,
        ds,
        ds_lo,
        v,
    }))
}

/// `std::min` of the loop bound `i <= tr.s && i <= lTot`.
#[inline]
fn cmin_usize(a: usize, b: usize) -> usize {
    if b < a {
        b
    } else {
        a
    }
}

/// The fourth s-derivatives' bounds of a span: `m4[f][k]` per face and jet, `uv4[f]` per
/// face's uv curves (infinite where not proved), then the same bounds' least values at the
/// span's middle (`pM4Lo`, `pUV4Lo`): no piece holding the middle bounds them lower.
pub type Fourth = (
    [[f64; MAX_E + 1]; 2],
    [f64; 2],
    [[f64; MAX_E + 1]; 2],
    [f64; 2],
);

/// The Taylor-form bound `sum_{j<M} (4+j)! |c[4+j](m)| (h/2)^j / j!` plus the remainder
/// `(4+M)! |c[4+M](box)| (h/2)^M / M!`, `M = SM_GN_TAYLOR`, of one coefficient row.
fn taylor_bound(mid: impl Fn(usize) -> f64, rem: f64, h: f64) -> f64 {
    let mut m = 0.0_f64;
    let mut fact = 24.0_f64;
    let mut pow = 1.0_f64;
    let mut jf = 1.0_f64;
    for j in 0..TAYLOR {
        if j > 0 {
            fact *= (4 + j) as f64;
            pow *= 0.5 * h;
            jf *= j as f64;
        }
        m += fact * mid(4 + j) * pow / jf;
    }
    fact *= (4 + TAYLOR) as f64;
    pow *= 0.5 * h;
    jf *= TAYLOR as f64;
    m += fact * rem * pow / jf;
    m
}

/// `sm_GnFourthOver`: the bound on the jets' fourth s-derivatives over `[a.s, b.s]`, per
/// face and jet (`m4[f][k]`, infinite where not proved), and on the feet's uv curves' times
/// the faces' `|S_u| + |S_v|` over the span's box (`uv4[f]`), in their Taylor form at the
/// interval's middle m: `c''''(s)` is within `sum_{j<M} c^(4+j)(m) (s - m)^j / j!` plus the
/// remainder `c^(4+M)(interval) (h/2)^M / M!`, `M = SM_GN_TAYLOR`, the derivatives at the
/// middle enclosed on that one point and the remainder's over the interval.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_fourth_over(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
) -> Result<Fourth> {
    let (fourth, _) = gn_fourth_over_series(
        k, faces_n, leader_n, faces, center, a, b, r, side, n, None, 0, None,
    )?;
    Ok(fourth)
}

/// The series `sm_GnFourthOver` hands a varying thumbweight's row terms (its `pF`, `pV`,
/// `pFm`, `pVm`, `pdMid`, 7102bc5): the feet's and V's over the span to order
/// `4 + SM_GN_TAYLOR` in s, at its middle to one less, and the middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceSeries {
    /// `pF`: the feet over the span.
    pub f: [VSer<Ivl>; 2],
    /// `pV`: V over the span.
    pub v: VSer<Ivl>,
    /// `pFm`: the feet at the middle.
    pub fm: [VSer<Ivl>; 2],
    /// `pVm`: V at the middle.
    pub vm: VSer<Ivl>,
    /// `pdMid`: the middle.
    pub mid: f64,
}

/// [`gn_fourth_over`] with the series of [`PieceSeries`] (`None` where they are not all
/// proved, as the fork's out-params are then not all set), and the fork's `pLaw` and
/// `lExtra` (0d43b4c): a ball whose radius varies, and the orders in s the series take
/// past `4 + SM_GN_TAYLOR` (the touch circle's m'); and its `pDefs` (c5b6bb7): the faces
/// that are blends by their definitions, enclosed over the boxes.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_fourth_over_series(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    law: Option<&GnLaw<'_>>,
    extra: usize,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<(Fourth, Option<PieceSeries>)> {
    let mut defs = defs;
    let none: (Fourth, Option<PieceSeries>) = (
        (
            [[f64::INFINITY; MAX_E + 1]; 2],
            [f64::INFINITY; 2],
            [[0.0; MAX_E + 1]; 2],
            [0.0; 2],
        ),
        None,
    );
    let h = b.s - a.s;
    let Some(e5) = gn_enclose(
        k,
        faces_n,
        leader_n,
        a,
        b,
        r,
        side,
        n,
        4 + TAYLOR + extra,
        law,
        defs.as_deref_mut(),
    )?
    else {
        return Ok(none);
    };
    // the middle's station, and the series there
    let mid = 0.5 * (a.s + b.s);
    let mut seed = [0.0_f64; 4];
    for (c, sc) in seed.iter_mut().enumerate() {
        let ac = a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
        let bc = b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
        *sc = 0.5 * (ac + bc);
    }
    let sm = gn_try!(center.evaluate_point(mid), none);
    let point = Trunc::new(0, 0)?;
    let tol = 1.0e-9 * smax(1.0, sm.length());
    let mid_r = gn_try!(law_series(law, mid, 0, false), none);
    let smid = gn_try!(
        gn_solve_station_counted(
            k,
            faces,
            center,
            mid,
            seed,
            r,
            side,
            point,
            tol,
            mid_r.as_ref()
        )
        .0,
        none
    );
    let Some(e4) = gn_enclose(
        k,
        faces_n,
        leader_n,
        &smid,
        &smid,
        r,
        side,
        n,
        4 + TAYLOR - 1 + extra,
        law,
        defs,
    )?
    else {
        return Ok(none);
    };
    let series = PieceSeries {
        f: e5.f,
        v: e5.v,
        fm: e4.f,
        vm: e4.v,
        mid,
    };
    let (mut m4, mut uv4, mut m4_lo, mut uv4_lo) = none.0;
    // the uv curves' fourth derivatives, in the same Taylor form, times the faces'
    // |S_u| + |S_v| over the span's box
    for (f, (uvf, &dsf)) in uv4.iter_mut().zip(&e5.ds).enumerate() {
        let mut dmax = 0.0_f64;
        for c in (2 * f)..(2 * f + 2) {
            let (Some(x4), Some(x5)) = (e4.x.get(c), e5.x.get(c)) else {
                return Ok((none.0, Some(series)));
            };
            let m = taylor_bound(|i| x4.at(i, 0).mag(), x5.at(4 + TAYLOR, 0).mag(), h);
            dmax = cmax(dmax, m);
        }
        *uvf = dmax * dsf;
    }
    // the same, least: the middle's 4! c[4] alone, times the faces' least |S_u| + |S_v|
    for (f, (uvl, &dsl)) in uv4_lo.iter_mut().zip(&e4.ds_lo).enumerate() {
        let mut dlo = 0.0_f64;
        for c in (2 * f)..(2 * f + 2) {
            let Some(x4) = e4.x.get(c) else {
                return Ok((none.0, Some(series)));
            };
            dlo = cmax(dlo, 24.0 * x4.at(4, 0).mig());
        }
        *uvl = dlo * dsl;
    }
    for ((m4f, f4), f5) in m4.iter_mut().zip(&e4.f).zip(&e5.f) {
        for (kk, m4fk) in m4f.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for (s4, s5) in f4.a.iter().zip(&f5.a) {
                // c^(4+j)(m) = (4+j)! F[4+j]: the Taylor polynomial's bound over
                // |s - m| <= h/2, and the remainder
                let m = taylor_bound(|i| s4.at(i, kk).mag(), s5.at(4 + TAYLOR, kk).mag(), h);
                sq += m * m;
            }
            *m4fk = crate::geom::vector3d::sqrt(sq);
        }
    }
    for (m4lf, f4) in m4_lo.iter_mut().zip(&e4.f) {
        for (kk, m4lfk) in m4lf.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for s4 in &f4.a {
                let m = 24.0 * s4.at(4, kk).mig();
                sq += m * m;
            }
            *m4lfk = crate::geom::vector3d::sqrt(sq);
        }
    }
    Ok(((m4, uv4, m4_lo, uv4_lo), Some(series)))
}

/// `sm_GnVarying`: a section whose thumbweight varies along s (the equal area one,
/// resolved per station): its order, `r1 r2`, and I's enclosures on their grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GnVarying {
    /// `n`.
    pub n: usize,
    /// `dRR`: `r1 r2`.
    pub rr: f64,
    /// `pCells`: I's enclosures, computed once per cell.
    pub cells: GnAreaCells,
}

/// `sm_GnRowTerms`: the largest `|(sum_ik dg_ikj c'_ik)|` (`x`) and `|W_j|` (`y`),
/// the least `W_j` (`w_lo`) per row, and the largest `|S - O|` (`r`).
#[derive(Clone, Debug, PartialEq)]
pub struct GnRowTerms {
    /// `X`.
    pub x: Vec<f64>,
    /// `Y`.
    pub y: Vec<f64>,
    /// `Wlo`.
    pub w_lo: Vec<f64>,
    /// `R`.
    pub r: f64,
}

/// `sm_Centred`: the coefficients `0..=SM_GN_TAY` of a series over a piece in their
/// Taylor form at its middle m: `c_i(s) = sum_{j < K - i} C(i + j, i) c_(i+j)(m) d^j +
/// C(K, i) c_K(xi) d^(K-i)`, `d = s - m`, from the middle's coefficients `mid[0..K-1]` and
/// the piece's `top` at order K.
#[must_use]
pub fn centred(mid: &[Ivl], top: Ivl, kk: usize, d: Ivl) -> Tay<Ivl> {
    let mut r = Tay::<Ivl>::zero();
    for i in 0..=TAY {
        let mut sum = Ivl::from(0.0);
        let mut pow = Ivl::from(1.0);
        let mut j = 0;
        while i + j < kk {
            if j > 0 {
                pow = pow * d;
            }
            let m = mid.get(i + j).copied().unwrap_or(IZ);
            sum = sum + Ivl::from(binom(i + j, i)) * m * pow;
            j += 1;
        }
        pow = pow * d;
        r.set(i, sum + Ivl::from(binom(kk, i)) * top * pow);
    }
    r
}

/// A vector series' coordinate q's coefficient `(i, k)`.
fn vat(v: &VSer<Ivl>, q: usize, i: usize, k: usize) -> Ivl {
    v.a.get(q).map_or(IZ, |c| c.at(i, k))
}

/// `sm_GnRowTermsOver`: the row terms over one piece, folded into `rt` (largest X, Y, R;
/// least W); `false` where w's enclosure or a term is not proved. `f`, `centre`: the
/// feet's and V's series over the piece (to order `K = 4 + SM_GN_TAYLOR` in s); `fm`,
/// `cm`: at its middle (to K - 1); `d`: s less the middle over the piece.
#[allow(clippy::too_many_arguments)]
pub fn gn_row_terms_over(
    vary: &mut GnVarying,
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    guess: f64,
    bar: &GnSectionFormT<Ivl>,
    o: [f64; 3],
    rt: &mut GnRowTerms,
) -> bool {
    const K: usize = 4 + TAYLOR;
    let n = vary.n;
    let [f0, f1] = f;
    let [fm0, fm1] = fm;
    // x = (F1 - V).(F2 - V) / (r1 r2): at the middle to order K - 1, over the piece at
    // order K
    let mut x_mid = [IZ; K];
    let mut x_top = IZ;
    for q in 0..3 {
        for i in 0..=K {
            for l in 0..=i {
                if i < K {
                    let t = (vat(fm0, q, l, 0) - vat(cm, q, l, 0))
                        * (vat(fm1, q, i - l, 0) - vat(cm, q, i - l, 0));
                    if let Some(xi) = x_mid.get_mut(i) {
                        *xi = *xi + t;
                    }
                } else {
                    x_top = x_top
                        + (vat(f0, q, l, 0) - vat(centre, q, l, 0))
                            * (vat(f1, q, i - l, 0) - vat(centre, q, i - l, 0));
                }
            }
        }
    }
    let inv_rr = Ivl::from(1.0) / Ivl::from(vary.rr);
    for xi in &mut x_mid {
        *xi = *xi * inv_rr;
    }
    x_top = x_top * inv_rr;
    let u = gn_half_angle(&centred(&x_mid, x_top, K, d));
    let tg = gn_target(&u);
    let phi = gn_area_sum::<Ivl>(n, Ivl::from(guess)) - tg.at(0);
    let mut w0 = IZ;
    let mut root = false;
    let mut rho = 1.0e-12 * (1.0 + guess.abs());
    let mut it = 0;
    while it < 40 && !root {
        let w = Ivl::new(guess - rho, guess + rho);
        let below_one = w.hi < 1.0;
        if !below_one {
            break;
        }
        let d1 = vary.cells.over(w).at(1);
        if d1.hi < 0.0 {
            let nn = Ivl::from(guess) - phi / d1;
            if nn.lo > w.lo && nn.hi < w.hi {
                w0 = nn;
                root = true;
            }
        }
        it += 1;
        rho *= 4.0;
    }
    if !root {
        return false;
    }
    let over_w0 = vary.cells.over(w0);
    let sw = gn_invert(&tg, &over_w0, w0);
    let form = gn_form_normalized(n, sw);
    // the jets c'_ik as series in s over the piece
    let mut c = [[[Tay::<Ivl>::zero(); 3]; MAX_E + 1]; 2];
    let mut r_max = 0.0_f64;
    for (cf, (ff, ffm)) in c.iter_mut().zip(f.iter().zip(fm)) {
        let mut sum = 0.0_f64;
        for (kk, ck) in cf.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for (q, ckq) in ck.iter_mut().enumerate() {
                let mut ac_mid = [IZ; K];
                for (i, a) in ac_mid.iter_mut().enumerate() {
                    *a = vat(ffm, q, i, kk);
                }
                *ckq = centred(&ac_mid, vat(ff, q, K, kk), K, d);
                if kk == 0 {
                    let oq = o.get(q).copied().unwrap_or(f64::NAN);
                    ckq.set(0, ckq.at(0) - Ivl::from(oq));
                }
                sq += ckq.at(0).mag() * ckq.at(0).mag();
            }
            sum += crate::geom::vector3d::sqrt(sq);
        }
        r_max = cmax(r_max, sum);
    }
    for (j, wj) in form.w.iter().enumerate() {
        let mut sq = 0.0_f64;
        for q in 0..3 {
            // the coefficient of sigma^4 of sum_ik dg_ikj c'_ik
            let mut s4 = IZ;
            for ((gf, bf), cf) in form.g.iter().zip(&bar.g).zip(&c) {
                for ((gk, bk), ck) in gf.iter().zip(bf).zip(cf).take(n + 1) {
                    let (Some(&g), Some(&b), Some(ckq)) = (gk.get(j), bk.get(j), ck.get(q)) else {
                        continue;
                    };
                    let mut dg = g;
                    dg.set(0, dg.at(0) - b);
                    for i in 0..=TAY {
                        s4 = s4 + dg.at(i) * ckq.at(TAY - i);
                    }
                }
            }
            sq += s4.mag() * s4.mag();
        }
        let dx = 24.0 * crate::geom::vector3d::sqrt(sq);
        let dy = 24.0 * wj.at(TAY).mag();
        let dwlo = wj.at(0).lo;
        if !(dx.is_finite() && dy.is_finite() && dwlo > 0.0) {
            return false;
        }
        let (Some(xj), Some(yj), Some(lj)) = (rt.x.get_mut(j), rt.y.get_mut(j), rt.w_lo.get_mut(j))
        else {
            return false;
        };
        *xj = cmax(*xj, dx);
        *yj = cmax(*yj, dy);
        *lj = cmin(*lj, dwlo);
    }
    if !r_max.is_finite() {
        return false;
    }
    rt.r = cmax(rt.r, r_max);
    true
}

/// The arc's homogeneous rows `H[j][q]` and weights `W[j]` ([`arc_rows`]).
pub type ArcRows<S> = ([[Tay<S>; 3]; 3], [Tay<S>; 3]);

/// `sm_ArcRows` (cc08236): the ball's circle as rows: the rational quadratic arc from the
/// rail `rho1` to `rho2` about the ball's centre `m`, its control points `rho1, P1, rho2`
/// and weights `1, w, 1`, `w = cos(theta / 2)` (theta the angle between the radii
/// `b_i = rho_i - m`), `P1 = m + (mid - m) / w^2` where the rails' tangents meet; as
/// homogeneous rows `H_j = W_j (P_j - O)` about a fixed point `o`. The series are in s,
/// over a ring S (`f64`, or intervals). `None` where a radius is not positive or the arc
/// is a half turn or more (no single arc).
#[must_use]
pub fn arc_rows<S: TayScalar + GnScalar>(
    rho1: &[Tay<S>; 3],
    rho2: &[Tay<S>; 3],
    m: &[Tay<S>; 3],
    o: [f64; 3],
) -> Option<ArcRows<S>> {
    let mut b11 = Tay::<S>::zero();
    let mut b22 = Tay::<S>::zero();
    let mut b12 = Tay::<S>::zero();
    for ((&r1, &r2), &mq) in rho1.iter().zip(rho2).zip(m) {
        let b1 = r1 - mq;
        let b2 = r2 - mq;
        b11 = b11 + b1 * b1;
        b22 = b22 + b2 * b2;
        b12 = b12 + b1 * b2;
    }
    if !b11.at(0).is_positive() || !b22.at(0).is_positive() {
        return None;
    }
    // cos^2(theta / 2) = (1 + cos theta) / 2
    let half = (Tay::<S>::lift(1.0) + b12 / tay_sqrt(&(b11 * b22))) * Tay::<S>::lift(0.5);
    if !half.at(0).is_positive() {
        return None; // a half turn or more: no single arc
    }
    let w = tay_sqrt(&half);
    let inv = Tay::<S>::lift(1.0) / half;
    let ws = [Tay::<S>::lift(1.0), w, Tay::<S>::lift(1.0)];
    let mut h = [[Tay::<S>::zero(); 3]; 3];
    for (q, (((&r1, &r2), &mq), &oq)) in rho1.iter().zip(rho2).zip(m).zip(&o).enumerate() {
        let so = Tay::<S>::lift(oq);
        let p1 = mq + ((r1 + r2) * Tay::<S>::lift(0.5) - mq) * inv;
        let [h0, h1, h2] = &mut h;
        if let (Some(a), Some(b), Some(c)) = (h0.get_mut(q), h1.get_mut(q), h2.get_mut(q)) {
            *a = r1 - so;
            *b = w * (p1 - so);
            *c = r2 - so;
        }
    }
    Some((h, ws))
}

/// `sm_ArcRowTermsOver` (cc08236): the arc's row terms over one piece
/// ([`gn_row_terms_over`]'s, every row varying), folded into `rt`; `false` where they are
/// not proved. `f`, `centre`: the rails' and the centre's series over the piece (to order
/// `K = 4 + SM_GN_TAYLOR`); `fm`, `cm`: at its middle (to K - 1); `d`: s less the middle
/// over the piece. `touch` (0d43b4c): the ball's radius varies, and the circle is the one
/// it touches the envelope along, about `m + ((rho1 - m) . m' / m' . m') m'` (`centre` and
/// `cm` one order more, for m').
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn arc_row_terms_over(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    o: [f64; 3],
    rt: &mut GnRowTerms,
    touch: bool,
) -> bool {
    const K: usize = 4 + TAYLOR;
    let mut rho = [[Tay::<Ivl>::zero(); 3]; 2];
    let mut mc = [Tay::<Ivl>::zero(); 3];
    for q in 0..3 {
        let mut ac_mid = [IZ; K];
        for (rf, (ff, ffm)) in rho.iter_mut().zip(f.iter().zip(fm)) {
            for (i, a) in ac_mid.iter_mut().enumerate() {
                *a = vat(ffm, q, i, 0);
            }
            if let Some(x) = rf.get_mut(q) {
                *x = centred(&ac_mid, vat(ff, q, K, 0), K, d);
            }
        }
        for (i, a) in ac_mid.iter_mut().enumerate() {
            *a = vat(cm, q, i, 0);
        }
        if let Some(x) = mc.get_mut(q) {
            *x = centred(&ac_mid, vat(centre, q, K, 0), K, d);
        }
    }
    if touch {
        // m' from the centre's coefficients one order up: (i + 1) c_(i+1)
        let mut dm = [Tay::<Ivl>::zero(); 3];
        for (q, dq) in dm.iter_mut().enumerate() {
            let mut ac_mid = [IZ; K];
            for (i, a) in ac_mid.iter_mut().enumerate() {
                *a = vat(cm, q, i + 1, 0) * Ivl::from((i + 1) as f64);
            }
            *dq = centred(
                &ac_mid,
                vat(centre, q, K + 1, 0) * Ivl::from((K + 1) as f64),
                K,
                d,
            );
        }
        let mut dd = Tay::<Ivl>::zero();
        let mut bd = Tay::<Ivl>::zero();
        for ((&dq, &mq), &r0q) in dm.iter().zip(&mc).zip(&rho[0]) {
            dd = dd + dq * dq;
            bd = bd + (r0q - mq) * dq;
        }
        if !dd.at(0).is_positive() {
            return false;
        }
        let kk = bd / dd;
        for (mq, &dq) in mc.iter_mut().zip(&dm) {
            *mq = *mq + kk * dq;
        }
    }
    let [rho0, rho1] = &rho;
    let Some((h, w)) = arc_rows(rho0, rho1, &mc, o) else {
        return false;
    };
    // |S - O| <= |m - O| + the ball's radius over the piece
    let mut d_mo = 0.0_f64;
    let mut d_rad = 0.0_f64;
    for ((mq, &oq), r0q) in mc.iter().zip(&o).zip(rho0) {
        let dd = (mq.at(0) - Ivl::from(oq)).mag();
        d_mo += dd * dd;
        let b = (r0q.at(0) - mq.at(0)).mag();
        d_rad += b * b;
    }
    let d_r = crate::geom::vector3d::sqrt(d_mo) + crate::geom::vector3d::sqrt(d_rad);
    for (j, (hj, wj)) in h.iter().zip(&w).enumerate() {
        let mut sq = 0.0_f64;
        for hq in hj {
            sq += hq.at(TAY).mag() * hq.at(TAY).mag();
        }
        let dx = 24.0 * crate::geom::vector3d::sqrt(sq);
        let dy = 24.0 * wj.at(TAY).mag();
        let dwlo = wj.at(0).lo;
        if !(dx.is_finite() && dy.is_finite() && dwlo > 0.0) {
            return false;
        }
        let (Some(xj), Some(yj), Some(lj)) = (rt.x.get_mut(j), rt.y.get_mut(j), rt.w_lo.get_mut(j))
        else {
            return false;
        };
        *xj = cmax(*xj, dx);
        *yj = cmax(*yj, dy);
        *lj = cmin(*lj, dwlo);
    }
    if !d_r.is_finite() {
        return false;
    }
    rt.r = cmax(rt.r, d_r);
    true
}

/// `sm_GnCertifySpan`: the certified bound on the Hermite span between two stations, or
/// infinity where the span is not proved. The Hermite remainder is `h^4 / 384` times the
/// jets' largest fourth s-derivative over the span, bounded on its pieces
/// ([`gn_fourth_over`] on each): the span cut in `2^L` equal pieces, L raised (to 6 at
/// most) until every piece is proved and, over the target, while that halves the bound, so
/// that a long span's bound follows its derivatives, not the width of one enclosure over
/// it. The feet's uv curves are Hermite interpolants of the same stations, and the face at
/// a uv curve's point is off the face's true foot by at most `|S_u, S_v|` times the uv
/// error, `h^4 / 384 |x''''|`: that bound counts too (an edge's curve against its face).
///
/// Every level's bound is at least the same sum with each term's least value at the
/// pieces' middles (every piece's enclosure holds its middle's value): once that is over
/// the target no finer level certifies the span, and the bound so far is returned for the
/// caller to halve the span.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_certify_span(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    target: f64,
    mut vary: Option<&mut GnVarying>,
    law: Option<&GnLaw<'_>>,
    mut defs: Option<&mut GnDefs<'_>>,
) -> Result<f64> {
    // the series' orders are checked once (the fork leaves them to its callers)
    Trunc::new(4 + TAYLOR, n)?;
    let mut bound_out = f64::INFINITY;
    let h = b.s - a.s;
    let mut last = f64::INFINITY;
    // the varying thumbweight's fixed one wb and point O: the first station's
    let mut bar = GnSectionFormT::<Ivl> {
        w: Vec::new(),
        g: [Vec::new(), Vec::new()],
    };
    let mut o = [0.0_f64; 3];
    // the ball's circle (n = 1, cc08236): its arc's rows all vary along s, about the first
    // station's centre
    let arc = n == 1;
    let rows_vary = vary.is_some() || arc;
    let l_jets = if arc { 0 } else { n }; // the jets graded on their own: the arc's rails only
    if let Some(v) = vary.as_deref() {
        if !(a.w.is_finite() && b.w.is_finite()) {
            return Err(SolidError::sm_err(OP));
        }
        bar = gn_form_normalized(v.n, Ivl::from(a.w));
        for (q, oq) in o.iter_mut().enumerate() {
            *oq = a.f[0].a.get(q).map_or(f64::NAN, |c| c.at(0, 0));
        }
    }
    if arc {
        bar.w = vec![Ivl::from(0.0); 3];
        for (q, oq) in o.iter_mut().enumerate() {
            *oq = a.v.a.get(q).map_or(f64::NAN, |c| c.at(0, 0));
        }
    }
    for level in 0..=6_u32 {
        let pieces = 1_usize << level;
        // the pieces' end stations
        let mut ends = Vec::new();
        ends.push(*a);
        let mut solved = true;
        let mut i = 1;
        while i < pieces && solved {
            let t = i as f64 / pieces as f64;
            let s = a.s + t * h;
            let mut seed = [0.0_f64; 4];
            for (c, sc) in seed.iter_mut().enumerate() {
                let ac = a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
                let bc = b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
                *sc = (1.0 - t) * ac + t * bc;
            }
            let sm = gn_try!(center.evaluate_point(s), bound_out);
            let point = Trunc::new(0, 0)?;
            let tol = 1.0e-9 * smax(1.0, sm.length());
            let end_r = gn_try!(law_series(law, s, 0, false), bound_out);
            match gn_solve_station_counted(
                k,
                faces,
                center,
                s,
                seed,
                r,
                side,
                point,
                tol,
                end_r.as_ref(),
            )
            .0
            {
                Ok(st) => ends.push(st),
                Err(_) => solved = false,
            }
            i += 1;
        }
        if !solved {
            continue;
        }
        ends.push(*b);
        let mut m4 = [[0.0_f64; MAX_E + 1]; 2];
        let mut uv4 = [0.0_f64; 2];
        let mut m4_lo = [[0.0_f64; MAX_E + 1]; 2];
        let mut uv4_lo = [0.0_f64; 2];
        let mut proved = true;
        let ld = bar.w.len();
        let mut rows = GnRowTerms {
            x: vec![0.0; ld],
            y: vec![0.0; ld],
            w_lo: vec![f64::INFINITY; ld],
            r: 0.0,
        };
        for pair in ends.windows(2) {
            if !proved {
                break;
            }
            let [ea, eb] = pair else {
                return Err(SolidError::sm_err(OP));
            };
            let ((pm, puv, pm_lo, puv_lo), series) = gn_try!(
                gn_fourth_over_series(
                    k,
                    faces_n,
                    leader_n,
                    faces,
                    center,
                    ea,
                    eb,
                    r,
                    side,
                    l_jets,
                    law,
                    // the touch circle of a varying ball takes the centre's
                    // s-derivative: one order more
                    usize::from(arc && law.is_some()),
                    defs.as_deref_mut(),
                ),
                bound_out
            );
            for (((m4f, pmf), uvf), &puvf) in m4.iter_mut().zip(&pm).zip(uv4.iter_mut()).zip(&puv) {
                for (m4fk, &pmfk) in m4f.iter_mut().zip(pmf).take(l_jets + 1) {
                    let finite = pmfk < f64::INFINITY;
                    if !finite {
                        proved = false;
                    }
                    *m4fk = cmax(*m4fk, pmfk);
                }
                let finite = puvf < f64::INFINITY;
                if !finite {
                    proved = false;
                }
                *uvf = cmax(*uvf, puvf);
            }
            for (lf, plf) in m4_lo.iter_mut().zip(&pm_lo) {
                for (l, &pl) in lf.iter_mut().zip(plf) {
                    *l = cmax(*l, pl);
                }
            }
            for (l, &pl) in uv4_lo.iter_mut().zip(&puv_lo) {
                *l = cmax(*l, pl);
            }
            if proved {
                if let Some(v) = vary.as_deref_mut() {
                    // the root's guess at the piece's middle: the stations' thumbweights,
                    // linearly
                    proved = match series {
                        Some(ps) => {
                            let t = (ps.mid - a.s) / h;
                            let d = Ivl::new(ea.s, eb.s) - Ivl::from(ps.mid);
                            let guess = (1.0 - t) * a.w + t * b.w;
                            gn_row_terms_over(
                                v, &ps.f, &ps.v, &ps.fm, &ps.vm, d, guess, &bar, o, &mut rows,
                            )
                        }
                        None => false,
                    };
                }
            }
            if proved && arc {
                proved = match series {
                    Some(ps) => {
                        let d = Ivl::new(ea.s, eb.s) - Ivl::from(ps.mid);
                        arc_row_terms_over(
                            &ps.f,
                            &ps.v,
                            &ps.fm,
                            &ps.vm,
                            d,
                            o,
                            &mut rows,
                            law.is_some(),
                        )
                    }
                    None => false,
                };
            }
        }
        if !proved {
            continue;
        }
        let mut bound = 0.0_f64;
        let mut da = 0.0_f64;
        for (m4f, &uvf) in m4.iter().zip(&uv4) {
            let mut sum = 0.0_f64;
            for &m4fk in m4f.iter().take(l_jets + 1) {
                sum += h * h * h * h / 384.0 * m4fk;
            }
            da = cmax(da, sum);
            bound = cmax(
                bound,
                cmax(
                    if rows_vary { 0.0 } else { sum },
                    h * h * h * h / 384.0 * uvf,
                ),
            );
        }
        // the arc: no fixed section, every row's remainder (the varying rows' bound with
        // A = 0)
        if arc {
            da = 0.0;
        }
        if rows_vary {
            let kh = h * h * h * h / 384.0;
            for (j, wj) in bar.w.iter().enumerate() {
                let (Some(&lj), Some(&yj), Some(&xj)) =
                    (rows.w_lo.get(j), rows.y.get(j), rows.x.get(j))
                else {
                    break;
                };
                let den = lj - kh * yj;
                let num = wj.hi * da + kh * xj + kh * yj * rows.r;
                let positive = den > 0.0;
                bound = if positive {
                    cmax(bound, num / den)
                } else {
                    f64::INFINITY
                };
                if !positive {
                    break;
                }
            }
        }
        if !bound.is_finite() {
            continue;
        }
        bound_out = cmin(bound_out, round_up(bound * (1.0 + 1.0e-12)));
        if bound_out <= target {
            break;
        }
        // the bound no finer level goes under: over the target, no level certifies the span
        // (the jets' least terms only where the bound takes the jets' sum, not with
        // varying rows, whose own least terms are not ported: the fork's f916a8d)
        let k4 = h * h * h * h / 384.0;
        let mut lower = 0.0_f64;
        for (m4lf, &uvl) in m4_lo.iter().zip(&uv4_lo) {
            let mut sum = 0.0_f64;
            for &m in m4lf.iter().take(l_jets + 1) {
                sum += k4 * m;
            }
            lower = cmax(lower, cmax(if rows_vary { 0.0 } else { sum }, k4 * uvl));
        }
        if round_down(lower * (1.0 - 1.0e-12)) > target {
            break;
        }
        // once proved, one finer level more only where the last one halved the bound
        if bound > 0.5 * last {
            break;
        }
        last = bound;
    }
    Ok(bound_out)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::super::station::{as_b_spline, nurbs_partials, vec3};
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::extent2d::Extent2d;
    use crate::math::{Vec2, Vec3};
    use crate::nurbs::b_spline_curve::NurbCircleParam;
    use crate::surface::cone::Cone;
    use crate::surface::plane::Plane;

    fn plane(origin: Vec3, x_axis: Vec3, y_axis: Vec3) -> Surface {
        Surface::from(Plane::new(
            origin,
            x_axis,
            y_axis,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    fn nurbs(k: &Kernel, s: &Surface) -> Nurbs {
        Nurbs::of(&as_b_spline(k, s).unwrap().unwrap()).unwrap()
    }

    fn station(
        k: &Kernel,
        faces: [&Surface; 2],
        leader: &BSplineCurve,
        s: f64,
        seed: [f64; 4],
        r: [f64; 2],
        side: [f64; 2],
    ) -> GnStation {
        let tr = Trunc::new(0, 0).unwrap();
        super::super::station::gn_solve_station(k, faces, leader, s, seed, r, side, tr, 1e-12)
            .unwrap()
    }

    fn contains(i: Ivl, x: f64) -> bool {
        i.lo <= x && x <= i.hi
    }

    /// Two planes at a square corner, the ball of radius r on the edge along y (the
    /// station test's closed form): the feet (tau r, L s, 0) and (0, L s, tau r) are linear
    /// in s and tau, so every fourth s-derivative is 0 and the span's bound is 0 to the
    /// enclosures' rounding. Checked against that closed form: the enclosed series hold
    /// the exact coefficients (`du1/deps = r`, `dv1/ds = L`, the box holds the feet over
    /// the span), every `m4`, `uv4` is below 1e-9, and the certified bound is in
    /// `[0, 1e-9]`.
    #[test]
    fn two_plane_span_bound_is_zero() {
        let k = Kernel::default();
        let f1 = plane(Vec3::ZERO, Vec3::X, Vec3::Y); // z = 0, (u, v) = (x, y)
        let f2 = plane(Vec3::ZERO, Vec3::Y, Vec3::Z); // x = 0, (u, v) = (y, z)
        let (r, len) = (2.0, 5.0);
        let leader =
            BSplineCurve::create_line_segment(3, Vec3::new(r, 0.0, r), Vec3::new(r, len, r))
                .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        assert_eq!((ln.p, ln.n, ln.u.len()), (1, 2, 4));
        let (n1, n2) = (nurbs(&k, &f1), nurbs(&k, &f2));
        let faces = [&f1, &f2];
        let (rr, side) = ([r, r], [1.0, 1.0]);
        let (sa, sb) = (0.2, 0.6);
        let a = station(&k, faces, &leader, sa, [r, len * sa, len * sa, r], rr, side);
        let b = station(&k, faces, &leader, sb, [r, len * sb, len * sb, r], rr, side);
        let n = 2;
        let e = gn_enclose(
            &k,
            [&n1, &n2],
            &ln,
            &a,
            &b,
            rr,
            side,
            n,
            4 + TAYLOR,
            None,
            None,
        )
        .unwrap()
        .expect("proved");
        assert!(contains(e.x[0].c[0][0], r) && contains(e.x[3].c[0][0], r));
        for q in [1, 2] {
            let x = e.x[q].c[0][0];
            assert!(x.lo <= len * sa && len * sb <= x.hi, "{q} {x:?}");
            assert!(contains(e.x[q].c[1][0], len), "{q}");
        }
        assert!(contains(e.x[0].c[0][1], r) && contains(e.x[3].c[0][1], r));
        // |S_u| + |S_v| of a unit plane is 2
        assert!((e.ds[0] - 2.0).abs() < 1e-12 && (e.ds[1] - 2.0).abs() < 1e-12);
        let (m4, uv4, _, _) =
            gn_fourth_over(&k, [&n1, &n2], &ln, faces, &leader, &a, &b, rr, side, n).unwrap();
        for f in 0..2 {
            assert!(uv4[f] >= 0.0 && uv4[f] < 1e-9, "{uv4:?}");
            for kk in 0..=n {
                assert!(m4[f][kk] >= 0.0 && m4[f][kk] < 1e-9, "{m4:?}");
            }
        }
        let bound = gn_certify_span(
            &k,
            [&n1, &n2],
            &ln,
            faces,
            &leader,
            &a,
            &b,
            rr,
            side,
            n,
            1e-6,
            None,
            None,
            None,
        )
        .unwrap();
        assert!((0.0..1e-9).contains(&bound), "{bound}");
    }

    /// The (u, v) of the face's net nearest to `p`, on a grid (a seed for Newton).
    fn nearest_uv(n: &Nurbs, p: Vec3) -> (f64, f64) {
        let (u0, u1) = (n.u[0], n.u[n.u.len() - 1]);
        let (v0, v1) = (n.v[0], n.v[n.v.len() - 1]);
        let mut best = (f64::INFINITY, u0, v0);
        for i in 0..=400 {
            for j in 0..=40 {
                let u = u0 + (u1 - u0) * f64::from(i) / 400.0;
                let v = v0 + (v1 - v0) * f64::from(j) / 40.0;
                let ku = span(&n.u, n.p, n.nu, u);
                let kv = span(&n.v, n.q, n.nv, v);
                let d = (vec3(nurbs_partials(n, ku, kv, u, v, 0).at(0, 0)) - p).length();
                if d < best.0 {
                    best = (d, u, v);
                }
            }
        }
        (best.1, best.2)
    }

    /// A cylinder of radius R about z standing on the plane z = 0, the ball of radius r
    /// outside it: the centre line L(s) is the circle of radius R + r at height r (a
    /// rational quadratic arc), and the feet are `F1 = (L_xy (R + tau r) / (R + r), 0)` on
    /// the plane and `F2 = (L_xy R / (R + r), tau r)` on the cylinder. So the jets' fourth
    /// s-derivatives are, exactly, `|c10''''| = |L''''|`, `|c11''''| = r / (R + r) |L''''|`,
    /// `|c20''''| = R / (R + r) |L''''|` and 0 for the rest, and the plane's uv curve is
    /// `L_xy` (`uv4 >= 2 max(|L_x''''|, |L_y''''|)`). Checked against that closed form, with
    /// L'''' from the leader's own evaluator: at the span's middle (one point, a tight
    /// enclosure) `4! c[4][k]` of every jet and of the plane's (u, v) holds the closed
    /// form's value; over the span every `m4`, `uv4` is finite and at least the closed
    /// form's maximum (sampled at 201 points); the certified bound is finite, below the
    /// target, at least `h^4 / 384` times the closed-form jets' sum, and level 0's (the
    /// pieces' maximum). The span's bounds are sound but loose (`m4[0][0]` about 36 times
    /// the closed form at h = 0.02: the series enclosed over an interval of s widen order
    /// by order, and the Taylor form's remainder carries that). A span five times as long
    /// fails Krawczyk's test in one enclosure and is proved by the pieces, its bound again
    /// finite, below the target and above the closed form's.
    #[test]
    fn cylinder_plane_span_bound_is_the_closed_form() {
        let k = Kernel::default();
        let (big_r, r) = (3.0, 1.0);
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let mut cyl = Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::X,
            Vec3::Y,
            big_r,
            0.0,
            360.0,
            10.0,
            false,
            false,
            false,
        );
        cyl.make_nurb(&k).unwrap();
        let fc = Surface::from(cyl);
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            big_r + r,
            10.0,
            80.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        // the sides: the plane's normal is +z, towards the ball; the cylinder's by its net
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = big_r / (big_r + r);
            Vec3::new(l.x * q, l.y * q, r)
        };
        let (cu, cv) = nearest_uv(&nc, foot_c(0.45));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = foot_c(0.45) - Vec3::new(0.0, 0.0, r);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let st = |s: f64| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            station(&k, faces, &leader, s, [l.x, l.y, u, v], rr, side)
        };
        let (sa, sb) = (0.4, 0.42);
        let (a, b) = (st(sa), st(sb));
        // the stations are the closed form's
        let fa = vec3(V3 {
            x: a.f[1].a[0].c[0][0],
            y: a.f[1].a[1].c[0][0],
            z: a.f[1].a[2].c[0][0],
        });
        assert!((fa - foot_c(sa)).length() < 1e-9);
        assert!((a.x[0].c[0][0] - lpt(sa).x).abs() < 1e-9);

        // L'''' over the span, sampled
        let (mut l4, mut l4x, mut l4y) = (0.0_f64, 0.0_f64, 0.0_f64);
        for i in 0..=200 {
            let s = sa + (sb - sa) * f64::from(i) / 200.0;
            let d = leader.evaluate(s, 4, true, false).unwrap();
            let d4 = d[4];
            assert!(d4.z.abs() < 1e-9);
            l4 = l4.max(Vec3::new(d4.x, d4.y, 0.0).length());
            l4x = l4x.max(d4.x.abs());
            l4y = l4y.max(d4.y.abs());
        }
        assert!(l4 > 1.0, "{l4}");
        let n = 2;
        // at the span's middle the jets' fourth derivatives are enclosed on one point: they
        // hold the closed form's, `(4! c[4][k])`
        let sm = 0.5 * (sa + sb);
        let ms = st(sm);
        let em = gn_enclose(
            &k,
            [&np, &nc],
            &ln,
            &ms,
            &ms,
            rr,
            side,
            n,
            4 + TAYLOR - 1,
            None,
            None,
        )
        .unwrap()
        .expect("proved at a point");
        let dm = leader.evaluate(sm, 4, true, false).unwrap()[4];
        let holds = |i: Ivl, x: f64| {
            let j = Ivl::from(24.0) * i;
            j.lo - 1e-9 <= x && x <= j.hi + 1e-9 && j.hi - j.lo < 1e-2
        };
        let (qp, qc) = (r / (big_r + r), big_r / (big_r + r));
        for (c, l) in [(0, dm.x), (1, dm.y), (2, 0.0)] {
            assert!(holds(em.f[0].a[c].c[4][0], l), "c10 {c}");
            assert!(holds(em.f[0].a[c].c[4][1], qp * l), "c11 {c}");
            assert!(holds(em.f[1].a[c].c[4][0], qc * l), "c20 {c}");
            assert!(holds(em.f[1].a[c].c[4][1], 0.0), "c21 {c}");
            assert!(holds(em.f[0].a[c].c[4][2], 0.0) && holds(em.f[1].a[c].c[4][2], 0.0));
        }
        // the plane's (u, v) is the foot's (x, y)
        assert!(holds(em.x[0].c[4][0], dm.x) && holds(em.x[1].c[4][0], dm.y));
        let (m4, uv4, _, _) =
            gn_fourth_over(&k, [&np, &nc], &ln, faces, &leader, &a, &b, rr, side, n).unwrap();
        let want = [
            [l4, r / (big_r + r) * l4, 0.0],
            [big_r / (big_r + r) * l4, 0.0, 0.0],
        ];
        for f in 0..2 {
            for kk in 0..=n {
                let (got, w) = (m4[f][kk], want[f][kk]);
                assert!(got.is_finite() && w <= got, "{f} {kk} {got} {w}");
            }
        }
        let wuv = 2.0 * l4x.max(l4y);
        assert!(wuv <= uv4[0] && uv4[0].is_finite(), "{} {wuv}", uv4[0]);
        assert!(uv4[1].is_finite());

        let h = sb - sa;
        let target = 1e-3;
        let bound = gn_certify_span(
            &k,
            [&np, &nc],
            &ln,
            faces,
            &leader,
            &a,
            &b,
            rr,
            side,
            n,
            target,
            None,
            None,
            None,
        )
        .unwrap();
        let jets = h * h * h * h / 384.0 * (want[0][0] + want[0][1]);
        assert!(bound.is_finite() && bound < target, "{bound}");
        assert!(
            bound >= jets && bound >= h * h * h * h / 384.0 * wuv,
            "{bound} {jets}"
        );
        // below the target at level 0: the bound is that level's, the pieces' maximum
        let lvl0 = h * h * h * h / 384.0
            * (m4[0][0] + m4[0][1] + m4[0][2])
                .max(m4[1][0] + m4[1][1] + m4[1][2])
                .max(uv4[0])
                .max(uv4[1]);
        assert!((bound - lvl0).abs() <= 1e-9 * lvl0, "{bound} {lvl0}");

        // a span five times as long: one enclosure over it fails Krawczyk's test, the
        // pieces prove it, and the bound still holds the closed form's
        let (lb, hl) = (st(0.5), 0.1);
        let mut l4l = 0.0_f64;
        for i in 0..=500 {
            let d = leader
                .evaluate(sa + hl * f64::from(i) / 500.0, 4, true, false)
                .unwrap();
            l4l = l4l.max(Vec3::new(d[4].x, d[4].y, 0.0).length());
        }
        let one = gn_enclose(
            &k,
            [&np, &nc],
            &ln,
            &a,
            &lb,
            rr,
            side,
            n,
            4 + TAYLOR,
            None,
            None,
        )
        .unwrap();
        assert!(one.is_none());
        let long = gn_certify_span(
            &k,
            [&np, &nc],
            &ln,
            faces,
            &leader,
            &a,
            &lb,
            rr,
            side,
            n,
            target,
            None,
            None,
            None,
        )
        .unwrap();
        let jl = hl * hl * hl * hl / 384.0 * (1.0 + r / (big_r + r)) * l4l;
        assert!(
            long.is_finite() && long < target && long >= jl,
            "{long} {jl}"
        );
    }
}
