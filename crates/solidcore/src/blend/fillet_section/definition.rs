//! A blend's definition evaluated exactly (opt-in, not upstream): one station solve at s
//! (the feet by Newton from the definition's seeds, then their series in s and in tau),
//! then the section exact in t, all as truncated Taylor series in `(sigma, dt)`, so the
//! partials are exact to rounding. The stations solved last are kept in the kernel's
//! station memo, so a row of points at one s (a grid's row, quadrature, an isocurve)
//! solves once.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletSection.cpp`
//! (`SmProceduralBlend::EvaluateDefinition` and its helpers, tangency-labs/smlib@014f66f,
//! c6d9b9a and 0136260, smlib#79 step 7, merged at 5e8c9c1).
//!
//! | Rust                                  | Fork (`SmFilletSection.cpp`)                     |
//! |---------------------------------------|--------------------------------------------------|
//! | [`T2Of`], [`T2`] and their operators  | `sm_T2Of`, `sm_T2`, `operator+ - *`, `sm_T2Scale` (c5b6bb7: over `double` or `sm_Ivl`) |
//! | [`T2Scalar`]                          | `sm_PositiveFinite`, `sm_PowOf` (c5b6bb7)        |
//! | [`t2_pow`]                            | `sm_T2Pow`                                       |
//! | [`TvOf`], [`Tv`] and their operators, [`tv_dot`], [`tv_of`] | `sm_TVT`, `sm_TV`, `sm_TVZeroOf`, `operator+ - *`, `sm_TVScale`, `sm_TVDot`, `sm_TVOf` |
//! | [`bern_taylor`]                       | `sm_BernTaylor`                                  |
//! | [`StationMemo`], [`STATION_MEMO`]     | `sm_StationMemo`, `t_asMemo`, `t_lMemoNext`, `SM_STATION_MEMO` |
//! | [`evaluate_definition`]               | `SmProceduralBlend::EvaluateDefinition` (its `bBefore`, c5b6bb7) |
//!
//! Deviations:
//! - **The memo.** The fork's `thread_local` 4-slot memo is a field of
//!   [`crate::kernel::Kernel`] (`blend_station_memo`), a `RefCell`: the definition is
//!   evaluated through `&Kernel`, as every surface is. It is borrowed only to look a
//!   station up and to store one, never across a solve, so a nested support's own
//!   evaluations use it meanwhile, as in the fork. A hit returns what a fresh solve from
//!   the same seeds gives (the key is the definition's serial, s and the truncation).
//! - **The memo's thumbweight.** The fork keeps a pointer to the slot it hit or filled
//!   and writes the equal area thumbweight's series there; here the slot's index. Nothing
//!   evaluates a surface between the two, so it is the same slot.
//! - **Out-params.** `sm_T2Pow` and `EvaluateDefinition` return theirs; the partials are
//!   `d[i][j]`, not one flat array.
//! - **Bounds.** `T2` reads 0 past its orders (the fork's operands always share them).

use core::cmp::Ordering;

use super::form::{gn_form, gn_form_t, Bern};
use super::interval::Ivl;
use super::section::gn_equal_area_w;
use super::series::{Ser, Trunc, VSer, MAX_E, MAX_K, MAX_S};
use super::station::{feet_on_knots, gn_solve_station_with, GnStation, StationOptions};
use super::tay::{gn_area_sum, gn_half_angle, gn_invert, gn_target, gn_wedge_cos, Ring, Tay, TAY};
use crate::error::{Result, SolidError};
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::surface::procedural_blend::{count_evaluation, count_solve, ProceduralBlend};

const OP: &str = "blend::fillet_section::definition::evaluate_definition";

/// The number type of `sm_T2Of`'s template (c5b6bb7): `double`, or `sm_Ivl` for a
/// definition enclosed over a box.
pub trait T2Scalar: Ring {
    /// `sm_PositiveFinite`: positive and finite (every number in it, for an interval).
    fn positive_finite(self) -> bool;
    /// `sm_PowOf`: `x^p` (`smos_Pow`); for an interval, p one of -1, -1/2 and 1/2 (the
    /// whole line for any other).
    #[must_use]
    fn pow_of(self, p: f64) -> Self;
}

impl T2Scalar for f64 {
    fn positive_finite(self) -> bool {
        self > 0.0 && self.is_finite()
    }
    fn pow_of(self, p: f64) -> Self {
        self.powf(p)
    }
}

impl T2Scalar for Ivl {
    fn positive_finite(self) -> bool {
        self.lo > 0.0 && self.hi.is_finite()
    }
    fn pow_of(self, p: f64) -> Self {
        if p == -1.0 {
            return Ivl::from(1.0) / self;
        }
        if p == 0.5 {
            return self.sqrt();
        }
        if p == -0.5 {
            return Ivl::from(1.0) / self.sqrt();
        }
        Ivl::whole()
    }
}

/// `sm_T2Of`: a truncated Taylor series in `(sigma, dt)` of orders `(s, e)`:
/// `c[i (e + 1) + j]` the coefficient of `sigma^i dt^j`, kept to total order `n` (`s + e`
/// unless truncated; the coefficients past it are zero, c5b6bb7).
#[derive(Clone, Debug, PartialEq)]
pub struct T2Of<Z> {
    s: usize,
    e: usize,
    n: usize,
    c: Vec<Z>,
}

/// `sm_T2`: [`T2Of`] in doubles.
pub type T2 = T2Of<f64>;

impl<Z: Ring> T2Of<Z> {
    /// `sm_T2Of(lS, lE, d)`: the constant d, kept to total order `s + e`.
    #[must_use]
    pub fn new(s: usize, e: usize, d: Z) -> Self {
        Self::with_order(s, e, d, s + e)
    }

    /// `sm_T2Of(lS, lE, d, lN)`: the constant d, kept to total order `n`.
    #[must_use]
    pub fn with_order(s: usize, e: usize, d: Z, n: usize) -> Self {
        let mut c = vec![Z::lift(0.0); (s + 1) * (e + 1)];
        if let Some(c0) = c.first_mut() {
            *c0 = d;
        }
        Self { s, e, n, c }
    }

    /// `at(i, j)`: the coefficient of `sigma^i dt^j` (0 past the orders).
    #[must_use]
    pub fn at(&self, i: usize, j: usize) -> Z {
        if i > self.s || j > self.e {
            return Z::lift(0.0);
        }
        self.c
            .get(i * (self.e + 1) + j)
            .copied()
            .unwrap_or_else(|| Z::lift(0.0))
    }

    /// Sets the coefficient of `sigma^i dt^j` (ignored past the orders).
    pub fn set(&mut self, i: usize, j: usize, v: Z) {
        if i > self.s || j > self.e {
            return;
        }
        if let Some(x) = self.c.get_mut(i * (self.e + 1) + j) {
            *x = v;
        }
    }

    /// The constant term `c[0]`.
    #[must_use]
    pub fn constant(&self) -> Z {
        self.c.first().copied().unwrap_or_else(|| Z::lift(0.0))
    }

    /// `a + b` (a's orders).
    #[must_use]
    pub fn add(&self, b: &Self) -> Self {
        let mut r = Self::with_order(self.s, self.e, Z::lift(0.0), self.n);
        for ((x, &p), &q) in r.c.iter_mut().zip(&self.c).zip(b.c.iter()) {
            *x = p + q;
        }
        r
    }

    /// `a - b` (a's orders).
    #[must_use]
    pub fn sub(&self, b: &Self) -> Self {
        let mut r = Self::with_order(self.s, self.e, Z::lift(0.0), self.n);
        for ((x, &p), &q) in r.c.iter_mut().zip(&self.c).zip(b.c.iter()) {
            *x = p - q;
        }
        r
    }

    /// `sm_T2Scale`: `a * d`.
    #[must_use]
    pub fn scale(&self, d: Z) -> Self {
        let mut r = Self::with_order(self.s, self.e, Z::lift(0.0), self.n);
        for (x, &p) in r.c.iter_mut().zip(&self.c) {
            *x = p * d;
        }
        r
    }

    /// `a * b`, truncated at a's orders and its total order.
    #[must_use]
    pub fn mul(&self, b: &Self) -> Self {
        let mut r = Self::with_order(self.s, self.e, Z::lift(0.0), self.n);
        for i in 0..=self.s {
            let mut j = 0;
            while j <= self.e && i + j <= self.n {
                let mut d = Z::lift(0.0);
                for i1 in 0..=i {
                    for j1 in 0..=j {
                        d = d + self.at(i1, j1) * b.at(i - i1, j - j1);
                    }
                }
                r.set(i, j, d);
                j += 1;
            }
        }
        r
    }
}

/// `sm_T2Pow`: `x^p` for a series whose constant term is positive: `x0^p (1 + z)^p`,
/// `z = x / x0 - 1`, summed as the binomial series in z, which has no constant term (so
/// its total order's terms are all there are).
///
/// # Errors
///
/// `SM_ERR` where the constant term is not positive and finite ("its section
/// degenerates at this point (a weight or a length is zero)").
pub fn t2_pow<Z: T2Scalar>(x: &T2Of<Z>, p: f64) -> Result<T2Of<Z>> {
    let x0 = x.constant();
    if !x0.positive_finite() {
        return Err(SolidError::sm_err(OP));
    }
    let mut z = x.scale(Z::lift(1.0) / x0);
    z.set(0, 0, Z::lift(0.0));
    let mut pow = T2Of::with_order(x.s, x.e, Z::lift(1.0), x.n);
    let mut sum = T2Of::with_order(x.s, x.e, Z::lift(1.0), x.n);
    let mut binom = Z::lift(1.0);
    for m in 1..=x.n {
        binom = binom * (Z::lift(p - (m - 1) as f64) / Z::lift(m as f64));
        pow = pow.mul(&z);
        sum = sum.add(&pow.scale(binom));
    }
    Ok(sum.scale(x0.pow_of(p)))
}

/// `sm_TVT`: a vector of three [`T2Of`]s.
#[derive(Clone, Debug, PartialEq)]
pub struct TvOf<Z> {
    /// The x, y and z series.
    pub a: [T2Of<Z>; 3],
}

/// `sm_TV`: [`TvOf`] in doubles.
pub type Tv = TvOf<f64>;

impl<Z: Ring> TvOf<Z> {
    /// `sm_TVZeroOf`: zero, of orders `(s, e)`.
    #[must_use]
    pub fn zero(s: usize, e: usize) -> Self {
        Self::zero_with_order(s, e, s + e)
    }

    /// Zero, of orders `(s, e)` kept to total order `n`.
    #[must_use]
    pub fn zero_with_order(s: usize, e: usize, n: usize) -> Self {
        let z = || T2Of::with_order(s, e, Z::lift(0.0), n);
        Self { a: [z(), z(), z()] }
    }

    /// `p + q`.
    #[must_use]
    pub fn add(&self, q: &Self) -> Self {
        let [a0, a1, a2] = &self.a;
        let [b0, b1, b2] = &q.a;
        Self {
            a: [a0.add(b0), a1.add(b1), a2.add(b2)],
        }
    }

    /// `p - q`.
    #[must_use]
    pub fn sub(&self, q: &Self) -> Self {
        let [a0, a1, a2] = &self.a;
        let [b0, b1, b2] = &q.a;
        Self {
            a: [a0.sub(b0), a1.sub(b1), a2.sub(b2)],
        }
    }

    /// `p * s`, s a series.
    #[must_use]
    pub fn times(&self, s: &T2Of<Z>) -> Self {
        let [a0, a1, a2] = &self.a;
        Self {
            a: [a0.mul(s), a1.mul(s), a2.mul(s)],
        }
    }

    /// `p * d` (`sm_TVScale`).
    #[must_use]
    pub fn scale(&self, d: Z) -> Self {
        let [a0, a1, a2] = &self.a;
        Self {
            a: [a0.scale(d), a1.scale(d), a2.scale(d)],
        }
    }
}

/// `sm_TVDot`: `p.a[0] q.a[0] + p.a[1] q.a[1] + p.a[2] q.a[2]`.
#[must_use]
pub fn tv_dot<Z: Ring>(p: &TvOf<Z>, q: &TvOf<Z>) -> T2Of<Z> {
    let [a0, a1, a2] = &p.a;
    let [b0, b1, b2] = &q.a;
    a0.mul(b0).add(&a1.mul(b1)).add(&a2.mul(b2))
}

/// `sm_TVOf`: a station's series in sigma (its `eps^k` coefficients), up to `s`, as a
/// [`Tv`] of orders `(s, e)`.
#[must_use]
pub fn tv_of(v: &VSer<f64>, k: usize, s: usize, e: usize) -> Tv {
    let mut r = Tv::zero(s, e);
    for (rc, vc) in r.a.iter_mut().zip(&v.a) {
        let mut i = 0;
        while i <= s && i <= MAX_S {
            rc.set(i, 0, vc.at(i, k));
            i += 1;
        }
    }
    r
}

/// `sm_BernTaylor`: `p^(j)(t0) / j!`, `j <= lj`, of the polynomial of Bernstein
/// coefficients `b`.
#[must_use]
pub fn bern_taylor(b: &[f64], t0: f64, lj: usize) -> Vec<f64> {
    let mut out = vec![0.0; lj + 1];
    let mut d: Bern = b.to_vec();
    let mut fact = 1.0_f64;
    let mut j_fact = 1.0_f64;
    let mut j = 0;
    while j <= lj && !d.is_empty() {
        if j > 0 {
            j_fact *= j as f64;
        }
        // de Casteljau at t0
        let mut w = d.clone();
        for r in 1..w.len() {
            for q in 0..(w.len() - r) {
                let (Some(&a), Some(&c)) = (w.get(q), w.get(q + 1)) else {
                    continue;
                };
                if let Some(x) = w.get_mut(q) {
                    *x = (1.0 - t0) * a + t0 * c;
                }
            }
        }
        if let (Some(o), Some(&w0)) = (out.get_mut(j), w.first()) {
            *o = fact * w0 / j_fact;
        }
        // the derivative's coefficients: deg (b_{q+1} - b_q)
        let deg = (d.len() - 1) as f64;
        let next: Bern = d
            .windows(2)
            .map(|p| match p {
                [a, b] => b - a,
                _ => 0.0,
            })
            .collect();
        fact *= deg;
        d = next;
        j += 1;
    }
    out
}

/// `SM_STATION_MEMO`: the stations the memo keeps.
pub const STATION_MEMO: usize = 4;

/// `sm_StationMemo`: one kept station.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct MemoSlot {
    /// `lSerial`: the definition's serial (0: empty).
    serial: u64,
    /// `dS`.
    s: f64,
    /// `ts`, `te`: the truncation it was solved to.
    ts: usize,
    te: usize,
    /// `bBefore` (c5b6bb7): the station's partials on the side before s.
    before: bool,
    /// `bSided`: and they differ from the side after it (s is one of the definition's
    /// stations).
    sided: bool,
    /// `sStation`.
    station: Option<GnStation>,
    /// `bW`, `sW`: the station's equal area thumbweight series, once resolved.
    w: Option<Tay<f64>>,
}

/// The stations the definitions on a kernel solved last (`t_asMemo`, `t_lMemoNext`): an
/// evaluation at the same s (another t) takes its station from here. Keyed by the
/// definition's serial, never reused, so a station is never another definition's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StationMemo {
    /// The slots filled so far, at most [`STATION_MEMO`] (the fork's empty slots, serial
    /// 0, never match: a serial is at least 1). On the heap: a station is kilobytes.
    slots: Vec<MemoSlot>,
    next: usize,
}

/// `SmProceduralBlend::EvaluateDefinition`: the blend's definition at `(s, t)` and its
/// partials to `ls` in s and `lt` in t: `d[i][j] = d^(i+j) S / ds^i dt^j`.
///
/// Where s is one of its stations (a support's knot line under a foot, a knot of the
/// leader or the law) the definition is only as smooth as they are there: its partials on
/// the side after s, or with `before` on the side before it (c5b6bb7). A foot on a knot
/// line takes its support's partials on the piece it moves into along s (comes from, with
/// `before`), the way the seed moves; the station is solved again with those sides where
/// that is not the piece a solve without sides takes. With
/// [`crate::kernel::UpstreamFixes::fillet_round_on_round_as_definition`] off, `before` is
/// not taken and no station is solved again (fork 5e8c9c1, before c5b6bb7).
///
/// One station solve: the feet by Newton from the seeds, then their series in s (and in
/// tau for the G^n jets) order by order, as the G^n section's stations are built. The
/// section is then exact in t: n >= 2, `sum_ik c_ik(s) phi_ik(t)`, `phi_ik` the
/// section's rational functions over its common weight (the equal area thumbweight
/// resolved as a series in s at this station, c6d9b9a); n = 1, the rational quadratic arc
/// from the first rail to the second on the circle the balls' envelope touches the ball
/// along: about `m + ((rho - m) . m' / m' . m') m'`, in the plane square to the spine's
/// tangent `m'`, weights 1, `cos(theta / 2)`, 1.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for partials past what the series hold; `SM_ERR` where the
/// radius law is not positive, the station does not solve, or the section degenerates;
/// those of the seeds', the law's and the leader's evaluations.
#[allow(clippy::too_many_lines)]
pub fn evaluate_definition(
    k: &Kernel,
    pb: &ProceduralBlend,
    s: f64,
    t: f64,
    ls: usize,
    lt: usize,
    before: bool,
) -> Result<Vec<Vec<Vec3>>> {
    let def = pb.definition();
    count_evaluation(k);
    // the side of s (c5b6bb7): only with WR-714's flag; before it the side after s
    let sides_taken = k.fixes.fillet_round_on_round_as_definition;
    let before = before && sides_taken;
    let n = def.order as usize;
    // the station: the jets to order n in tau (G^n), or the rails and the centre one
    // order more in s (the circle's m')
    let (ts, te) = if n >= 2 { (ls, n) } else { (ls + 1, 0) };
    if ls > 64 || lt > 64 || ts > MAX_S || te > MAX_E || ts + te + 1 > MAX_K {
        return Err(SolidError::invalid_input(
            OP,
            "partials of this order are past what its definition's series hold",
        ));
    }
    let tr = Trunc::new(ts, te)?;
    let mut seed = [0.0_f64; 4];
    for (f, sd) in def.seeds.iter().enumerate() {
        let w = sd.evaluate_point(s)?;
        if let Some(x) = seed.get_mut(2 * f) {
            *x = w.x;
        }
        if let Some(x) = seed.get_mut(2 * f + 1) {
            *x = w.y;
        }
    }
    let mut r = def.offsets;
    let mut law_ser = Ser::constant(0.0_f64);
    let mut law: Option<&Ser<f64>> = None;
    if let Some(curve) = &def.law {
        let d = curve.evaluate(s, ts, !before, false)?;
        let mut fact = 1.0_f64;
        for i in 0..=ts {
            if i > 0 {
                fact *= i as f64;
            }
            let di = d.get(i).ok_or_else(|| SolidError::sm_err(OP))?;
            law_ser.set(i, 0, di.x / fact);
        }
        let d0 = d.first().ok_or_else(|| SolidError::sm_err(OP))?;
        if d0.x.partial_cmp(&0.0) != Some(Ordering::Greater) {
            // "its radius law is not positive here"
            return Err(SolidError::sm_err(OP));
        }
        r = [d0.x, d0.x];
        law = Some(&law_ser);
    }

    // the memo: a station solved at this s, to this truncation, for this definition
    let serial = def.serial;
    let mut slot: Option<usize> = None;
    let mut at: Option<GnStation> = None;
    let mut memo_w: Option<Tay<f64>> = None;
    {
        let memo = k.blend_station_memo.borrow();
        for (m, e) in memo.slots.iter().enumerate() {
            if e.serial == serial
                && e.s == s
                && e.ts == ts
                && e.te == te
                && (!e.sided || e.before == before)
            {
                at = e.station;
                memo_w = e.w;
                slot = Some(m);
                break;
            }
        }
    }
    let at = if let Some(st) = at {
        st
    } else {
        let l = def.leader.evaluate_point(s)?;
        let [s0, s1] = &def.supports;
        let faces = [s0.as_ref(), s1.as_ref()];
        // (a nested support's own evaluations use the memo meanwhile: the station is
        // kept only once solved)
        let tol = 1.0e-9 * smax(1.0, l.length());
        // (its jets of order n and up the face curves' normal parts for n >= 2, 3bc54a4)
        let top = if def.order >= 2 {
            def.order as usize
        } else {
            0
        };
        let opts = StationOptions {
            law,
            top,
            sides: None,
            before,
        };
        let (solved, steps) =
            gn_solve_station_with(k, faces, &def.leader, s, seed, r, def.sides, tr, tol, opts);
        count_solve(k, steps);
        let mut st = solved?.0;
        let mut sided = false;
        if sides_taken {
            // a foot on a knot line of its support: the support's partials there on the
            // piece the foot moves into along s (comes from, `before`), as the stations'
            // series were taken; the way it moves is the seed's (the stations' feet and
            // their s-derivatives, interpolated)
            let mut dir = [0.0_f64; 4];
            for (f, sd) in def.seeds.iter().enumerate() {
                let w = sd.evaluate(s, 1, !before, false)?;
                let w1 = w.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?;
                if let Some(x) = dir.get_mut(2 * f) {
                    *x = w1.x;
                }
                if let Some(x) = dir.get_mut(2 * f + 1) {
                    *x = w1.y;
                }
            }
            let on = st.x.map(|x| x.at(0, 0));
            let (sides, other) = feet_on_knots(k, faces, on, dir, before, tr.s() + tr.e() + 1)?;
            if other {
                let opts = StationOptions {
                    law,
                    top,
                    sides: Some(sides),
                    before,
                };
                let (solved, steps) = gn_solve_station_with(
                    k,
                    faces,
                    &def.leader,
                    s,
                    on,
                    r,
                    def.sides,
                    tr,
                    tol,
                    opts,
                );
                count_solve(k, steps);
                st = solved?.0;
            }
            // the side matters only at one of the definition's stations: a foot on a knot
            // line, or s a knot of the leader or the law; elsewhere the station is the same
            // from either side
            sided = sides.iter().any(|&q| q != 0);
            // (the leader's derivatives to order s + 1 enter, its unit tangent's; the law's
            // to order s; a curve is C^(p - multiplicity) at a knot, its derivatives to
            // that order the same either side)
            let along = [(Some(&def.leader), ts + 1), (def.law.as_ref(), ts)];
            for (curve, used) in along {
                if sided {
                    break;
                }
                let Some(curve) = curve else {
                    continue;
                };
                let (knots, mult) = curve.knots(None)?;
                let deg = curve.degree()?;
                let mut m = 1;
                while m + 1 < knots.len() && m < mult.len() {
                    let (Some(&km), Some(&mm)) = (knots.get(m), mult.get(m)) else {
                        break;
                    };
                    let on_knot = (s - km).abs() <= 1.0e-9 * (1.0 + km.abs());
                    // used > p - multiplicity, in signed arithmetic
                    if on_knot && used + mm > deg {
                        sided = true;
                    }
                    m += 1;
                }
            }
        }
        let mut memo = k.blend_station_memo.borrow_mut();
        let m = memo.next;
        memo.next = (memo.next + 1) % STATION_MEMO;
        let entry = MemoSlot {
            serial,
            s,
            ts,
            te,
            before,
            sided,
            station: Some(st),
            w: None,
        };
        if let Some(e) = memo.slots.get_mut(m) {
            *e = entry;
        } else {
            memo.slots.push(entry);
        }
        slot = Some(m);
        st
    };

    let mut sum = Tv::zero(ls, lt);
    if n >= 2 && def.thumbweight == super::section::GN_THUMBWEIGHT_EQUAL_AREA {
        // the equal area thumbweight of this station's own wedge, as a series in s (the
        // builder's rule, sm_GnStationThumbweight, to every order asked): w(s) solves
        // I(w) = target(u(s)), u half the angle between the ball's radii to its contacts
        if ls > TAY {
            return Err(SolidError::invalid_input(
                OP,
                "partials of this order are past what its thumbweight's series holds",
            ));
        }
        // (kept with the station: the root is the costly part, and a row of points
        // shares it)
        let [r0, r1] = def.offsets;
        let ws = if let Some(w) = memo_w {
            w
        } else {
            let u = gn_half_angle(&gn_wedge_cos::<f64>(&at.f, &at.v, r0 * r1, ls));
            let w0 = gn_equal_area_w(n, u.at(0))?;
            let d = gn_area_sum(n, Tay::of_terms(w0, 1.0));
            if d.at(1).partial_cmp(&0.0) != Some(Ordering::Less) {
                // "the section's area does not fall with its thumbweight"
                return Err(SolidError::sm_err(OP));
            }
            let ws = gn_invert(&gn_target(&u), &d, w0);
            if let Some(m) = slot {
                let mut memo = k.blend_station_memo.borrow_mut();
                if let Some(e) = memo.slots.get_mut(m) {
                    e.w = Some(ws);
                }
            }
            ws
        };
        let form = gn_form_t::<Tay<f64>>(n, ws);
        // a row of the form in (sigma, dt): sum_j g_j(sigma) B_j(t0 + dt)
        let deg = form.w.len();
        let basis: Vec<Vec<f64>> = (0..deg)
            .map(|j| {
                let mut unit = vec![0.0; deg];
                if let Some(x) = unit.get_mut(j) {
                    *x = 1.0;
                }
                bern_taylor(&unit, t, lt)
            })
            .collect();
        let row = |g: &[Tay<f64>]| -> T2 {
            let mut sr = T2::new(ls, lt, 0.0);
            for (gj, bj) in g.iter().zip(&basis) {
                for i in 0..=ls {
                    for (q, &bq) in bj.iter().enumerate().take(lt + 1) {
                        sr.set(i, q, sr.at(i, q) + gj.at(i) * bq);
                    }
                }
            }
            sr
        };
        let inv_w = t2_pow(&row(&form.w), -1.0)?;
        for (ff, gf) in at.f.iter().zip(&form.g) {
            for (kk, gk) in gf.iter().enumerate().take(n + 1) {
                sum = sum.add(&tv_of(ff, kk, ls, lt).times(&row(gk).mul(&inv_w)));
            }
        }
    } else if n >= 2 {
        let form = gn_form(n, def.thumbweight);
        let mut w = T2::new(0, lt, 0.0);
        for (j, &x) in bern_taylor(&form.w, t, lt).iter().enumerate() {
            w.set(0, j, x);
        }
        let inv_w = t2_pow(&w, -1.0)?;
        for (ff, gf) in at.f.iter().zip(&form.g) {
            for (kk, gk) in gf.iter().enumerate().take(n + 1) {
                let mut sn = T2::new(0, lt, 0.0);
                for (j, &x) in bern_taylor(gk, t, lt).iter().enumerate() {
                    sn.set(0, j, x);
                }
                let phi1 = sn.mul(&inv_w);
                let mut phi = T2::new(ls, lt, 0.0);
                for j in 0..=lt {
                    phi.set(0, j, phi1.at(0, j));
                }
                sum = sum.add(&tv_of(ff, kk, ls, lt).times(&phi));
            }
        }
    } else {
        // the centre to one order more, for its tangent m'
        let m1 = tv_of(&at.v, 0, ls + 1, lt);
        let mut sd = Tv::zero(ls, lt);
        let mut sm = Tv::zero(ls, lt);
        for ((dc, mc), m1c) in sd.a.iter_mut().zip(sm.a.iter_mut()).zip(&m1.a) {
            for i in 0..=ls {
                dc.set(i, 0, (i + 1) as f64 * m1c.at(i + 1, 0));
                mc.set(i, 0, m1c.at(i, 0));
            }
        }
        let [f0, f1] = &at.f;
        let rho1 = tv_of(f0, 0, ls, lt);
        let rho2 = tv_of(f1, 0, ls, lt);
        let inv_dd = t2_pow(&tv_dot(&sd, &sd), -1.0)?;
        // the touch circle's centre: the ball's moved along m' by
        // (rho - m) . m' / m' . m' (zero where r is constant)
        let mc = sm.add(&sd.times(&tv_dot(&rho1.sub(&sm), &sd).mul(&inv_dd)));
        let b1 = rho1.sub(&mc);
        let b2 = rho2.sub(&mc);
        let i1 = t2_pow(&tv_dot(&b1, &b1), -0.5)?;
        let i2 = t2_pow(&tv_dot(&b2, &b2), -0.5)?;
        // cos^2(theta / 2)
        let half = tv_dot(&b1, &b2)
            .mul(&i1)
            .mul(&i2)
            .add(&T2::new(ls, lt, 1.0))
            .scale(0.5);
        let w = t2_pow(&half, 0.5)?;
        let inv_w2 = t2_pow(&half, -1.0)?;
        // the middle control point, where the tangents at the rails meet
        let p1 = mc.add(&rho1.add(&rho2).scale(0.5).sub(&mc).times(&inv_w2));
        let t0 = t;
        let table = [
            [(1.0 - t0) * (1.0 - t0), -2.0 * (1.0 - t0), 1.0],
            [2.0 * t0 * (1.0 - t0), 2.0 - 4.0 * t0, -2.0],
            [t0 * t0, 2.0 * t0, 1.0],
        ];
        let mut bs = [
            T2::new(ls, lt, 0.0),
            T2::new(ls, lt, 0.0),
            T2::new(ls, lt, 0.0),
        ];
        for (b, row) in bs.iter_mut().zip(&table) {
            for (j, &x) in row.iter().enumerate().take(lt + 1) {
                b.set(0, j, x);
            }
        }
        let [b0, bm, b2s] = &bs;
        let wb1 = w.mul(bm);
        let inv_den = t2_pow(&b0.add(&wb1).add(b2s), -1.0)?;
        sum = rho1
            .times(b0)
            .add(&p1.times(&wb1))
            .add(&rho2.times(b2s))
            .times(&inv_den);
    }
    let mut out = vec![vec![Vec3::ZERO; lt + 1]; ls + 1];
    let mut fi = 1.0_f64;
    for (i, row) in out.iter_mut().enumerate() {
        if i > 0 {
            fi *= i as f64;
        }
        let mut fj = 1.0_f64;
        for (j, slot) in row.iter_mut().enumerate() {
            if j > 0 {
                fj *= j as f64;
            }
            let [x, y, z] = &sum.a;
            *slot = Vec3::new(x.at(i, j), y.at(i, j), z.at(i, j)) * (fi * fj);
        }
    }
    Ok(out)
}

/// `smos_Max(a, b)`: `(a > b) ? a : b`.
#[inline]
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}
