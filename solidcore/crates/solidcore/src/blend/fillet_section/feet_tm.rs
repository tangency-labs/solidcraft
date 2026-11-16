//! The feet over a piece of a G^n section, enclosed by Taylor models in s (not upstream,
//! #14405): the certificate's [`super::certificate::gn_enclose_over`] with every quantity a
//! [`TmN`] in `t = (s - m) / delta` over the piece `[m - delta, m + delta]` instead of one
//! interval over it.
//!
//! The interval enclosure takes the feet's box over the whole piece, the faces' partials
//! over that box and the leader's derivatives over the piece as independent ranges, and so
//! each Taylor coefficient of the feet over the piece. Those ranges move together along s;
//! the residual of each order sums terms that cancel, and its enclosure is orders of
//! magnitude over the coefficient's true range (on a rational face, `1e3` to `1e8` times at
//! a piece of a few thousandths, which the Hermite remainder then carries). Here the feet
//! are a polynomial in t plus a remainder box (a tube about the polynomial), and the faces'
//! partials and the leader at them are Taylor models of t too: the common motion cancels,
//! and only the tube's radius, the models' truncation and rounding are left over.
//!
//! - **The tube (Krawczyk's operator for each t).** `p0(t)`, the feet's Taylor polynomial at
//!   the middle (from its point enclosure, any polynomial would do), and `Y`, the inverse
//!   of the system's Jacobian there. For a box `R` about 0, where `-Y G(p0(t), t)` and
//!   `I - Y J(p0(t) + R, t)` lie in `D` and `C` for every t (their models' ranges), and
//!   `D + C R` lies in R's interior, the operator `K_t = p0(t) - Y G(p0(t), t) +
//!   (I - Y J(X_t, t)) (X_t - p0(t))` maps `X_t = p0(t) + R` into its interior for every t:
//!   by Krawczyk's theorem (Rump's form) G has exactly one root in `X_t`, and it lies in
//!   `p0(t) + (D + C R)`. The contraction `q = |C|` (row sums) is required below 1, as the
//!   interval enclosure requires it. The tube at the middle (t = 0) must hold the middle's
//!   point enclosure: its root there is then the middle's, and the one root of every `X_t`
//!   (none on its boundary) is the feet's branch continued from the middle.
//! - **The coefficients, order by order.** Each order's coefficients solve `J x = -R_i`,
//!   `R_i` the residual of the lower orders (exact algebra of the series, enclosed by the
//!   models for every t): `x = b + C x`, `b = -Y R_i`. `P`, a polynomial close to the root
//!   (`b + C P` iterated on the models' middles), leaves the defect `D = b + C P - P`; for
//!   every t, `|x - P| <= |D| / (1 - q)` (rows' maximum), so `x` lies in
//!   `P + D + C [-e, e]`, `e` that bound.
//! - **Rounding.** Every model's coefficients and remainder are outward-rounded intervals
//!   ([`TmN`]); the faces' partials are their polynomial pieces' Taylor polynomials at the
//!   tube's middle (exact point derivatives) about a local origin (the middle's point:
//!   `S - c = (A - c w) / w`, so the quotient's rounding is the local size's, not the
//!   coordinates'), the leader's likewise.
//!
//! The faces' and the leader's polynomial pieces are those [`super::certificate::gn_enclose`]
//! takes (the middle's, continued: the stations hold their knots). A face that is a blend by
//! its definition is not enclosed here (the caller keeps the interval enclosure).
//!
//! | Rust                       | Interval form (`certificate.rs`) |
//! |----------------------------|-----------------------------------|
//! | [`gn_enclose_tm`]          | [`super::certificate::gn_enclose_over`] |
//! | [`nurbs_partials_tm`]      | [`super::certificate::nurbs_partials_over_piece`] |
//! | [`curve_derivs_tm`]        | [`super::certificate::curve_derivs_over`] |
//! | [`leader_series_tm`]       | [`super::certificate::leader_series_over`] |
//! | [`law_series_tm`]          | [`super::certificate::law_series_over`] |
//!
//! Deviations: not the fork's (no counterpart); every loop has a fixed count (8 tube steps,
//! 4 polynomial steps per order), no recursion. `I - Y J`, `-Y G` and the sums `D + C R`,
//! `b + C P` are the crate's one Krawczyk operator's pieces ([`crate::geom::krawczyk`]'s
//! `contraction`, `start_minus_y` and `plus_c_times`, over [`Ft`]), in the order this
//! module's own copies had (tangency-labs/solidcraft#15140).

use super::certificate::{inverse4, Enclosure, GnLaw, NurbsCurve, SpanAt};
use super::form::binom;
use super::interval::{cmax, round_down, round_up, Ivl};
use super::series::{Ser, Trunc, VSer, MAX_S, V3};
use super::station::{
    ders_basis, gn_evaluate, gn_jacobian, gn_top_jets, knot, span, GnIncremental, GnSystem, Nurbs,
    Partials,
};
use super::taylor_model::TmN;
use crate::error::{Result, SolidError};
use crate::geom::krawczyk::{contraction, plus_c_times, start_minus_y};
use crate::kernel::Kernel;

const OP: &str = "blend::fillet_section::feet_tm";

/// The number of coefficients of the feet's Taylor models (degree one less): enough that a
/// piece's truncation, `(delta / rho)^N` of the feet's size for a radius of convergence
/// rho, is far below the bounds the certificate takes from them, and few enough that the
/// series' many products stay cheap. Measured on `issue_2406` r1 (#14405, goldens profile):
/// 3, 4 and 6 coefficients took 3.47, 3.44 and 4.24 s.
pub const FEET_TM_N: usize = 4;

/// A Taylor model of the feet's enclosure.
pub type Ft = TmN<FEET_TM_N>;

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// `SM_GN_TRY`: a failed evaluation proves nothing (`Ok(None)`); a cancel ends the
/// operation.
macro_rules! tm_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            Err(_) => return Ok(None),
        }
    };
}

fn zero_v() -> V3<Ft> {
    V3 {
        x: Ft::constant(IZ),
        y: Ft::constant(IZ),
        z: Ft::constant(IZ),
    }
}

/// The homogeneous control point `(x w, y w, z w, w)` at `li`, NaN past the net.
fn pw4(pw: &[f64], li: usize) -> [Ivl; 4] {
    let at = |i: usize| Ivl::from(pw.get(i).copied().unwrap_or(f64::NAN));
    [at(li), at(li + 1), at(li + 2), at(li + 3)]
}

/// The local origin of a homogeneous point `h`: its Euclidean point's middle, or 0 where
/// that is not finite.
fn origin(h: &[Ivl; 4]) -> [f64; 3] {
    let [x, y, z, w] = *h;
    let wm = w.mid();
    let c = [x.mid() / wm, y.mid() / wm, z.mid() / wm];
    if c.iter().all(|v| v.is_finite()) {
        c
    } else {
        [0.0; 3]
    }
}

/// A NURBS face's partials `P[a][b]`, `a + b <= lk`, at the feet `(u(t), v(t))` given as
/// Taylor models: the face's polynomial piece `(ku, kv)`, continued (as
/// [`super::certificate::nurbs_partials_over_piece`] takes it), enclosed for every t. The
/// homogeneous piece is its Taylor polynomial at `(um, vm)`, the middles of the models'
/// constant terms, from exact point derivatives there; `(u - um, v - vm)` are models, so
/// is every monomial; the quotient (the NURBS book's A4.4) is taken about the local origin
/// `c`, the point at `(um, vm)`: `S - c = (A - c w) / w`.
///
/// # Errors
///
/// `SM_ERR` where the weight is not proved positive over the models' ranges.
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_tm(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    u: Ft,
    v: Ft,
    lk: usize,
) -> Result<Partials<Ft>> {
    let um = u.c.first().map_or(f64::NAN, |c| c.mid());
    let vm = v.c.first().map_or(f64::NAN, |c| c.mid());
    if !(um.is_finite() && vm.is_finite()) {
        return Err(SolidError::blend_failed(
            OP,
            "the feet's models have no finite midpoint",
        ));
    }
    let nu = ders_basis(&cn.u, ku, cn.p, Ivl::from(um), cn.p);
    let nv = ders_basis(&cn.v, kv, cn.q, Ivl::from(vm), cn.q);
    let (la, lb) = (cn.p, cn.q);
    let nb =
        |m: &[Vec<Ivl>], a: usize, i: usize| m.get(a).and_then(|r| r.get(i)).copied().unwrap_or(IZ);
    // homogeneous point derivatives at the middle, Hm[a][b] (x, y, z, w)
    let mut hm = vec![vec![[IZ; 4]; lb + 1]; la + 1];
    for (a, row) in hm.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            let mut h = [IZ; 4];
            for i in 0..=cn.p {
                for j in 0..=cn.q {
                    // ku >= p and kv >= q (sm_Span)
                    let li = 4 * ((ku + i - cn.p) * cn.nv + (kv + j - cn.q));
                    let c = nb(&nu, a, i) * nb(&nv, b, j);
                    for (he, x) in h.iter_mut().zip(pw4(&cn.pw, li)) {
                        *he = *he + c * x;
                    }
                }
            }
            *hab = h;
        }
    }
    // about the local origin c: A - c w and its partials (the weight's unchanged)
    let c0 = origin(hm.first().and_then(|r| r.first()).unwrap_or(&[IZ; 4]));
    for row in &mut hm {
        for hab in row.iter_mut() {
            let [_, _, _, w] = *hab;
            for (he, &cq) in hab.iter_mut().zip(&c0) {
                *he = *he - Ivl::from(cq) * w;
            }
        }
    }
    // (u - um)^i / i! and (v - vm)^j / j!, then their products
    let du = u.add_ivl(Ivl::from(-um));
    let dv = v.add_ivl(Ivl::from(-vm));
    let mut pu = vec![Ft::constant(Ivl::from(1.0)); la + 1];
    let mut pv = vec![Ft::constant(Ivl::from(1.0)); lb + 1];
    for i in 1..=la {
        let prev = pu.get(i - 1).copied().unwrap_or_else(Ft::whole);
        if let Some(x) = pu.get_mut(i) {
            *x = (prev * du).scale(Ivl::from(1.0) / Ivl::from(i as f64));
        }
    }
    for j in 1..=lb {
        let prev = pv.get(j - 1).copied().unwrap_or_else(Ft::whole);
        if let Some(x) = pv.get_mut(j) {
            *x = (prev * dv).scale(Ivl::from(1.0) / Ivl::from(j as f64));
        }
    }
    let mono: Vec<Vec<Ft>> = pu
        .iter()
        .map(|&a| pv.iter().map(|&b| a * b).collect())
        .collect();
    let mono_at = |i: usize, j: usize| {
        mono.get(i)
            .and_then(|r| r.get(j))
            .copied()
            .unwrap_or_else(|| Ft::constant(IZ))
    };
    // the homogeneous partials A_ab, w_ab at the feet, a + b <= lk (0 past the degrees)
    let mut av = vec![vec![[Ft::constant(IZ); 4]; lk + 1]; lk + 1];
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut h = [Ft::constant(IZ); 4];
            for i in a..=la {
                for j in b..=lb {
                    let m = mono_at(i - a, j - b);
                    let hij = hm.get(i).and_then(|r| r.get(j)).copied().unwrap_or([IZ; 4]);
                    for (he, &x) in h.iter_mut().zip(&hij) {
                        *he = *he + m.scale(x);
                    }
                }
            }
            if let Some(x) = av.get_mut(a).and_then(|r| r.get_mut(b)) {
                *x = h;
            }
        }
    }
    let hat = |a: usize, b: usize, e: usize| {
        av.get(a)
            .and_then(|r| r.get(b))
            .and_then(|h| h.get(e))
            .copied()
            .unwrap_or_else(|| Ft::constant(IZ))
    };
    let w00 = hat(0, 0, 3);
    if !w00.range().is_positive() {
        // "SmGnSectionFSG: a face's weights are not positive over a box"
        return Err(SolidError::blend_failed(
            OP,
            "a face's weights are not positive over a box",
        ));
    }
    let inv_w = w00.recip();
    let mut out = Partials::<Ft>::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut v3 = [hat(a, b, 0), hat(a, b, 1), hat(a, b, 2)];
            for i in 0..=a.min(la) {
                for j in 0..=b.min(lb) {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let wij = hat(i, j, 3);
                    let c = wij.scale(Ivl::from(binom(a, i) * binom(b, j)));
                    let pij = out.at(a - i, b - j);
                    for (vq, pq) in v3.iter_mut().zip([pij.x, pij.y, pij.z]) {
                        *vq = *vq - c * pq;
                    }
                }
            }
            let [x, y, z] = v3.map(|q| q * inv_w);
            out.set(a, b, V3 { x, y, z });
        }
    }
    let p0 = out.at(0, 0);
    out.set(
        0,
        0,
        V3 {
            x: p0.x.add_ivl(Ivl::from(c0[0])),
            y: p0.y.add_ivl(Ivl::from(c0[1])),
            z: p0.z.add_ivl(Ivl::from(c0[2])),
        },
    );
    Ok(out)
}

/// A NURBS curve's derivatives `D[0..=ld]` at `s = m + delta t`, `t` in `[-1, 1]`, as
/// Taylor models: its polynomial piece `piece` (continued), the homogeneous curve's Taylor
/// polynomial in t at m from exact point derivatives (a polynomial of degree p: rounding
/// only), the quotient about the local origin `c`, the point at m.
///
/// # Errors
///
/// `SM_ERR` where the weight is not proved positive.
pub fn curve_derivs_tm(
    cn: &NurbsCurve,
    m: f64,
    delta: f64,
    ld: usize,
    piece: usize,
) -> Result<Vec<V3<Ft>>> {
    let p = cn.p;
    let has_span = knot(&cn.u, piece as isize) < knot(&cn.u, piece as isize + 1);
    if !has_span || piece < p {
        return Err(SolidError::blend_failed(
            OP,
            "the piece is not a span of the curve",
        ));
    }
    let nb = ders_basis(&cn.u, piece, p, Ivl::from(m), p);
    let mut am = vec![[IZ; 4]; p + 1];
    for (d, ad) in am.iter_mut().enumerate() {
        let mut h = [IZ; 4];
        for j in 0..=p {
            let nj = nb.get(d).and_then(|r| r.get(j)).copied().unwrap_or(IZ);
            for (he, x) in h.iter_mut().zip(pw4(&cn.pw, 4 * (piece + j - p))) {
                *he = *he + nj * x;
            }
        }
        *ad = h;
    }
    let c0 = origin(am.first().unwrap_or(&[IZ; 4]));
    for ad in &mut am {
        let [_, _, _, w] = *ad;
        for (he, &cq) in ad.iter_mut().zip(&c0) {
            *he = *he - Ivl::from(cq) * w;
        }
    }
    // A^(d)(m + delta t) = sum_j A^(d + j)(m) (delta t)^j / j!
    let dl = Ivl::from(delta);
    let mut hd = vec![[Ft::constant(IZ); 4]; ld + 1];
    for (d, hdd) in hd.iter_mut().enumerate() {
        for (e, he) in hdd.iter_mut().enumerate() {
            let mut coeffs = Vec::new();
            let mut pow = Ivl::from(1.0);
            let mut f = 1.0_f64;
            for j in 0..=(p.saturating_sub(d)) {
                if d + j > p {
                    break;
                }
                if j > 0 {
                    pow = pow * dl;
                    f *= j as f64;
                }
                let a = am.get(d + j).and_then(|h| h.get(e)).copied().unwrap_or(IZ);
                coeffs.push(a * pow * (Ivl::from(1.0) / Ivl::from(f)));
            }
            if d <= p {
                *he = Ft::from_poly(&coeffs, IZ);
            }
        }
    }
    let w0 = hd.first().map_or_else(Ft::whole, |&[_, _, _, w]| w);
    if !w0.range().is_positive() {
        return Err(SolidError::blend_failed(
            OP,
            "the curve's weight is not proved positive",
        ));
    }
    let inv_w = w0.recip();
    let mut out: Vec<V3<Ft>> = vec![zero_v(); ld + 1];
    for d in 0..=ld {
        let [hx, hy, hz, _] = hd.get(d).copied().unwrap_or_else(|| [Ft::constant(IZ); 4]);
        let mut v = [hx, hy, hz];
        for i in 1..=d {
            let wi = hd
                .get(i)
                .map_or_else(|| Ft::constant(IZ), |&[_, _, _, w]| w);
            let c = wi.scale(Ivl::from(binom(d, i)));
            let prev = out.get(d - i).copied().unwrap_or_else(zero_v);
            for (vq, pq) in v.iter_mut().zip([prev.x, prev.y, prev.z]) {
                *vq = *vq - c * pq;
            }
        }
        let [x, y, z] = v.map(|q| q * inv_w);
        if let Some(o) = out.get_mut(d) {
            *o = V3 { x, y, z };
        }
    }
    if let Some(o) = out.first_mut() {
        o.x = o.x.add_ivl(Ivl::from(c0[0]));
        o.y = o.y.add_ivl(Ivl::from(c0[1]));
        o.z = o.z.add_ivl(Ivl::from(c0[2]));
    }
    Ok(out)
}

/// The leader's point and unit tangent at `s = m + delta t` as series in sigma to `tr.s`,
/// their coefficients Taylor models in t ([`super::certificate::leader_series_over`]'s,
/// on the piece `piece` or, `None`, the one m lies in).
///
/// # Errors
///
/// Those of [`curve_derivs_tm`]; `SM_ERR` where the tangent is not proved non-zero.
pub fn leader_series_tm(
    leader: &NurbsCurve,
    m: f64,
    delta: f64,
    tr: Trunc,
    piece: Option<usize>,
) -> Result<(VSer<Ft>, VSer<Ft>)> {
    let sigma = Trunc::new(tr.s(), 0)?;
    let piece = piece.unwrap_or_else(|| span(&leader.u, leader.p, leader.n, m));
    let d = curve_derivs_tm(leader, m, delta, tr.s() + 1, piece)?;
    let mut l = VSer::constant(zero_v());
    let mut d1 = l.clone();
    let mut fact = 1.0_f64;
    for j in 0..=tr.s() {
        if j > 0 {
            fact *= j as f64;
        }
        let f = Ivl::from(1.0) / Ivl::from(fact);
        let dj = d.get(j).copied().unwrap_or_else(zero_v);
        let dj1 = d.get(j + 1).copied().unwrap_or_else(zero_v);
        let cl = [dj.x, dj.y, dj.z];
        let cd = [dj1.x, dj1.y, dj1.z];
        for ((la, da), (&x, &y)) in l.a.iter_mut().zip(d1.a.iter_mut()).zip(cl.iter().zip(&cd)) {
            la.set(j, 0, x.scale(f));
            da.set(j, 0, y.scale(f));
        }
    }
    let inv = d1.dot(&d1, sigma).inv_sqrt(sigma)?;
    let t = d1.times(&inv, sigma);
    Ok((l, t))
}

/// A radius law's r at `s = m + delta t` to order `order` in sigma, its coefficients
/// Taylor models in t ([`super::certificate::law_series_over`]'s).
///
/// # Errors
///
/// Those of [`curve_derivs_tm`]; `SM_ERR` where r is not proved positive.
pub fn law_series_tm(law: &GnLaw<'_>, m: f64, delta: f64, order: usize) -> Result<Ser<Ft>> {
    let mut r = Ser::constant(Ft::constant(IZ));
    let piece = span(&law.n.u, law.n.p, law.n.n, m);
    let d = curve_derivs_tm(&law.n, m, delta, order, piece)?;
    let mut fact = 1.0_f64;
    for i in 0..=order {
        if i > 0 {
            fact *= i as f64;
        }
        let di = d.get(i).ok_or_else(|| {
            SolidError::blend_failed(OP, "the radius law has no derivative of this order")
        })?;
        r.set(i, 0, di.x.scale(Ivl::from(1.0) / Ivl::from(fact)));
    }
    if !r.at(0, 0).range().is_positive() {
        return Err(SolidError::blend_failed(
            OP,
            "the radius is not proved positive",
        ));
    }
    Ok(r)
}

/// The system over the partials `p` at the feet's models.
fn system_tm<'a>(
    p: &'a [Partials<Ft>; 2],
    leader: &VSer<Ft>,
    tangent: &VSer<Ft>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&'a Ser<Ft>>,
) -> GnSystem<'a, Ft> {
    let [p0, p1] = p;
    GnSystem {
        p: [p0, p1],
        leader: leader.clone(),
        tangent: tangent.clone(),
        r,
        side,
        hold: false,
        law,
    }
}

/// `I - Y J` ([`contraction`] over models), its models' ranges, and its row sums' largest
/// magnitude over those ranges (`q`).
fn contraction_ranges(y: &[[f64; 4]; 4], jac: &[[Ft; 4]; 4]) -> ([[Ft; 4]; 4], [[Ivl; 4]; 4], f64) {
    let (cm, q) = contraction(y, jac);
    (cm, cm.map(|row| row.map(|c| c.range())), q)
}

/// `-Y g` as models ([`start_minus_y`] from 0).
fn minus_y_times(y: &[[f64; 4]; 4], g: &[Ft; 4]) -> [Ft; 4] {
    start_minus_y([Ft::constant(IZ); 4], y, g)
}

/// The feet's series over `at.s`, to order `order_s` in s and `n` in tau, enclosed by
/// Taylor models in s (see the [module](self)); `None` where the tube is not proved (or an
/// evaluation fails: nothing is proved). `mid`: the piece's middle, the models' expansion
/// point; `seed`: the feet's series there (a point enclosure: their middles make the
/// tube's polynomial). The faces' pieces are `at.ku`, `at.kv`, the leader's `at.leader`
/// (as [`super::certificate::gn_enclose`] sets them). The returned [`Enclosure`] holds each
/// model's range over the piece.
///
/// # Errors
///
/// `InvalidInput` when `order_s` or `n` exceeds the series' size.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn gn_enclose_tm(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    at: &SpanAt,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    mid: f64,
    seed: &[Ser<Ivl>; 4],
) -> Result<Option<Enclosure>> {
    crate::profile::profile_scope!(k, "blend::certificate::gn_enclose_tm");
    k.check_cancelled(OP)?;
    let tr = Trunc::new(order_s, n)?;
    // [mid - delta, mid + delta] holds the piece
    let delta = round_up(cmax(at.s.hi - mid, mid - at.s.lo));
    if !(delta.is_finite() && delta >= 0.0 && mid.is_finite()) {
        return Ok(None);
    }
    let law_r = match law {
        Some(l) => Some(tm_try!(law_series_tm(l, mid, delta, order_s))),
        None => None,
    };
    let law_r = law_r.as_ref();
    let (leader, tangent) = tm_try!(leader_series_tm(leader_n, mid, delta, tr, at.leader));
    let [ku0, ku1] = at.ku;
    let [kv0, kv1] = at.kv;
    let partials = |x: &[Ft; 4], lk: usize| -> Result<[Partials<Ft>; 2]> {
        let [n0, n1] = faces_n;
        let [x0, x1, x2, x3] = *x;
        Ok([
            nurbs_partials_tm(n0, ku0, kv0, x0, x1, lk)?,
            nurbs_partials_tm(n1, ku1, kv1, x2, x3, lk)?,
        ])
    };

    // the tube's polynomial: the middle's coefficients, x_j(m) (delta t)^j
    let mut p0 = [Ft::constant(IZ); 4];
    for (pc, sc) in p0.iter_mut().zip(seed) {
        let mut coeffs = [0.0_f64; FEET_TM_N];
        let mut pow = 1.0_f64;
        for (j, cj) in coeffs.iter_mut().enumerate().take(MAX_S + 1) {
            if j > 0 {
                pow *= delta;
            }
            *cj = sc.at(j, 0).mid() * pow;
        }
        if !coeffs.iter().all(|c| c.is_finite()) {
            return Ok(None);
        }
        *pc = Ft::from_points(&coeffs);
    }
    let point = Trunc::new(0, 0)?;
    // G at the polynomial for every t, and the preconditioner: Y, the inverse of the
    // Jacobian there at t = 0
    let pz = tm_try!(partials(&p0, 2));
    let sx0 = p0.map(Ser::constant);
    let sys0 = system_tm(&pz, &leader, &tangent, r, side, law_r);
    let jac0 = tm_try!(gn_jacobian(&sys0, &sx0));
    let jm = jac0.map(|row| row.map(|j| j.c.first().map_or(f64::NAN, |c| c.mid())));
    let y = tm_try!(inverse4(&jm));
    let (g0, _) = tm_try!(gn_evaluate(&sys0, &sx0, point));
    let dm = minus_y_times(&y, &g0.map(|g| g.at(0, 0)));
    let dr = dm.map(|d| d.range());
    // the tube: a box R about the polynomial whose Krawczyk image lies inside it
    let (_, cr0, q0) = contraction_ranges(&y, &jac0);
    let next_radii = |cr: &[[Ivl; 4]; 4], q: f64| -> Option<[f64; 4]> {
        let contracts = q < 1.0;
        if !contracts {
            return None;
        }
        // K's radii are at most d + |C| w: w = (I - |C|)^-1 d holds them; a half more, to
        // clear rounding and the change of C with the tube
        let d = dr.map(Ivl::mag);
        let mut w = d;
        for _ in 0..200 {
            let mut wn = [0.0_f64; 4];
            for ((wni, &di), ci) in wn.iter_mut().zip(&d).zip(cr) {
                *wni = di;
                for (cij, &wj) in ci.iter().zip(&w) {
                    *wni += cij.mag() * wj;
                }
            }
            w = wn;
        }
        let mut rr = [0.0_f64; 4];
        for ((ri, &wi), pc) in rr.iter_mut().zip(&w).zip(&p0) {
            let m0 = pc.c.first().map_or(f64::NAN, |c| c.mid());
            *ri = 1.5 * wi + 1.0e-13 * (1.0 + m0.abs());
        }
        rr.iter().all(|x| x.is_finite()).then_some(rr)
    };
    let Some(mut rr) = next_radii(&cr0, q0) else {
        return Ok(None);
    };
    let mut proved: Option<[Ivl; 4]> = None;
    for _ in 0..8 {
        let mut tube = p0;
        for (tc, &ri) in tube.iter_mut().zip(&rr) {
            *tc = tc.with_rem(Ivl::new(-ri, ri));
        }
        let pt = tm_try!(partials(&tube, 2));
        let sys_t = system_tm(&pt, &leader, &tangent, r, side, law_r);
        let jac = tm_try!(gn_jacobian(&sys_t, &tube.map(Ser::constant)));
        let (_, cr, q) = contraction_ranges(&y, &jac);
        let contracts = q < 1.0;
        if !contracts {
            return Ok(None);
        }
        // K's remainder about the polynomial: D + C R
        let kk = plus_c_times(dr, &cr, &rr.map(|rj| Ivl::new(-rj, rj)));
        let inside = kk.iter().zip(&rr).all(|(v, &ri)| v.lo > -ri && v.hi < ri);
        if inside {
            proved = Some(kk);
            break;
        }
        // a larger tube (never a smaller one) from this one's contraction
        let Some(next) = next_radii(&cr, q) else {
            return Ok(None);
        };
        for (ri, &ni) in rr.iter_mut().zip(&next) {
            *ri = cmax(*ri, ni);
        }
    }
    let Some(kk) = proved else {
        return Ok(None);
    };
    // the branch: the tube at the middle (t = 0, the polynomial its constant) holds the
    // middle's root, so the tube's one root for every t is the feet's continued from there
    for ((pc, sc), &ri) in p0.iter().zip(seed).zip(&rr) {
        let c0 = pc.c.first().copied().unwrap_or_else(Ivl::whole) + Ivl::new(-ri, ri);
        let e = sc.at(0, 0);
        if !(c0.lo <= e.lo && e.hi <= c0.hi) {
            return Ok(None);
        }
    }
    // the root, for every t: p0(t) plus K's remainder (within R)
    let mut x0 = p0;
    for ((xc, &ki), &ri) in x0.iter_mut().zip(&kk).zip(&rr) {
        let lo = cmax(ki.lo, -ri);
        let hi = if ki.hi < ri { ki.hi } else { ri };
        *xc = xc.with_rem(Ivl::new(lo, hi));
    }

    // the coefficients, order by order: x = b + C x, b = -Y R, C over the root's tube
    let lk = tr.s() + tr.e() + 1;
    let pk = tm_try!(partials(&x0, lk.max(2)));
    let sysk = system_tm(&pk, &leader, &tangent, r, side, law_r);
    let jk = tm_try!(gn_jacobian(&sysk, &x0.map(Ser::constant)));
    let (cm, cr, q) = contraction_ranges(&y, &jk);
    // 1 - q rounded down: the quotient rounded up is then at least the exact one
    let gap = round_down(1.0 - q);
    if !(gap.is_finite() && gap > 0.0) {
        return Ok(None);
    }
    let mut sx = x0.map(Ser::constant);
    // each order's G takes the lower orders' series as the last one left them: they are
    // kept, and only the coefficients x's new ones reach are computed (bit for bit a
    // whole evaluation's)
    let mut series = GnIncremental::new(&sysk);
    let eps_solved = if n >= 1 { tr.e().min(n - 1) } else { tr.e() };
    for tot in 1..=(tr.s() + tr.e()) {
        let eps = eps_solved.min(tot);
        if tot > tr.s() + eps {
            continue;
        }
        let before = series.computed();
        let evaluated = series.evaluate(&sx, tr.computing(tot, eps));
        k.work_counters
            .add_feet_tm_coefficients(series.computed().saturating_sub(before));
        let (sg, _, _, _) = tm_try!(evaluated);
        for i in 0..=tr.s().min(tot) {
            let kk = tot - i;
            // the section's jets of order n are the face curve's
            if kk > tr.e() || (n >= 1 && kk >= n) {
                continue;
            }
            let b = minus_y_times(&y, &sg.each_ref().map(|g| g.at(i, kk)));
            // P: close to the root, b + C P iterated on the middles
            let mut pp = b.map(|x| x.mid_poly());
            for _ in 0..4 {
                pp = plus_c_times(b, &cm, &pp).map(|x| x.mid_poly());
            }
            // the defect D = b + C P - P, and |x - P| <= |D| / (1 - q)
            let mut b_minus_p = b;
            for (bp, &pi) in b_minus_p.iter_mut().zip(&pp) {
                *bp = *bp - pi;
            }
            let dd = plus_c_times(b_minus_p, &cm, &pp).map(|v| v.range());
            let mut e = 0.0_f64;
            for di in &dd {
                e = cmax(e, di.mag());
            }
            let e = round_up(e / gap);
            if !e.is_finite() {
                return Ok(None);
            }
            let rems = plus_c_times(dd, &cr, &[Ivl::new(-e, e); 4]);
            for ((sxc, &pc), &rem) in sx.iter_mut().zip(&pp).zip(&rems) {
                sxc.set(i, kk, pc.with_rem(rem));
            }
        }
    }
    let before = series.computed();
    let evaluated = series.evaluate(&sx, tr);
    k.work_counters
        .add_feet_tm_coefficients(series.computed().saturating_sub(before));
    let (_, mut f, v, nrm) = tm_try!(evaluated);
    gn_top_jets(n, tr, &nrm, &mut f);
    let range_ser = |s: &Ser<Ft>| {
        let mut o = Ser::constant(IZ);
        for i in 0..=tr.s() {
            for kk in 0..=tr.e() {
                o.set(i, kk, s.at(i, kk).range());
            }
        }
        o
    };
    let range_vser = |s: &VSer<Ft>| VSer {
        a: s.a.each_ref().map(range_ser),
    };
    let mut ds = [[0.0_f64; 2]; 2];
    let mut ds_lo = [[0.0_f64; 2]; 2];
    for ((dsf, dsl), pf) in ds.iter_mut().zip(ds_lo.iter_mut()).zip(&pk) {
        let (su, sv) = (pf.at(1, 0), pf.at(0, 1));
        let len = |c: V3<Ft>, f: fn(Ivl) -> f64| {
            let (x, y, z) = (f(c.x.range()), f(c.y.range()), f(c.z.range()));
            crate::geom::vector3d::sqrt(x * x + y * y + z * z)
        };
        *dsf = [len(su, Ivl::mag), len(sv, Ivl::mag)];
        *dsl = [len(su, Ivl::mig), len(sv, Ivl::mig)];
    }
    Ok(Some(Enclosure {
        f: f.each_ref().map(range_vser),
        x: sx.each_ref().map(range_ser),
        ds,
        ds_lo,
        v: range_vser(&v),
    }))
}
