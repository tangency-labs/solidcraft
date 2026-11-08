//! The continuation of a blend's section past its range: the closed form by which a
//! distance proved on the section's `[t0, t1]` grows over a continuation of the same
//! chord or circle (docs/definitions-and-certificates.md 6.4). One construction for its two
//! callers: a blend extended across its section ([`crate::surface::extend`]) and a net
//! with pieces past the section re-certified ([`super::recertify`]).
//!
//! With `W` the section's weight and `V` the net's (polynomials in t of degrees `d_S` and
//! `d_B`), `E = V (W S) - W (V B)` is a polynomial of degree `m = d_S + d_B` with
//! `|E| <= max W max V eps` on `[t0, t1]`; past it, by Chebyshev's extremal property,
//! `|E(t)| <= max |E| T_m(x)`, `x = |2 t - t0 - t1| / (t1 - t0)`, so
//! `|S - B|(t) <= eps T_m(x) (max W max V) / (min W min V)` ([`growth`]).
//!
//! Not upstream. Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::nurbs::math::{gamma, NOW};
use crate::nurbs::srf_basic::Surface as Net;
use crate::surface::extend::ExtendRefusal;

const OP: &str = "surface::extend::Surface::extended_by_distance";

/// The section's degree in t: the chord's 1, the circle's 2, a G^n section's `3 n + 1`.
pub(crate) fn section_degree(order: u32) -> usize {
    match order {
        0 => 1,
        1 => 2,
        n => 3 * n as usize + 1,
    }
}

/// `T_m(x) (max W max V) / (min W min V)`, rounded up: the growth of a distance proved on
/// the section's range, at the farthest point `x` (`|2 t - t0 - t1| / (t1 - t0)`) of its
/// continuation, `m` the sum of the section's and the net's degrees; `w = (max, min)` of
/// the section's weight ([`section_weight_range`]) and `v = (max, min)` of the net's
/// ([`net_weights`]).
pub(crate) fn growth(m: usize, x: f64, (w_hi, w_lo): (f64, f64), (v_hi, v_lo): (f64, f64)) -> f64 {
    chebyshev(m, x) * (w_hi * v_hi) / (w_lo * v_lo) * (1.0 + gamma(4 * m + 16))
}

/// The section's weight `W(t)` of order `order`: its largest value over `certified`, the t
/// range the distance is proved on, and its least over `ext`, the extension's, rounded
/// outward. The chord's is 1; the circle's `1 - 2 t (1 - t) (1 - c)` is at most
/// `1 + 2 max(0, t (t - 1))` (1 on `[0, 1]`) and at least 1 off `(0, 1)`, above 1/2 on it;
/// a G^n section's is [`gn_weight_range`] over `thumbweight()`, the thumbweight's proved
/// `[lo, hi]` (asked for only by a G^n section).
///
/// # Errors
///
/// Those of `thumbweight` and [`gn_weight_range`].
pub(crate) fn section_weight_range(
    order: u32,
    certified: [f64; 2],
    ext: [f64; 2],
    thumbweight: impl FnOnce() -> Result<[f64; 2]>,
) -> Result<(f64, f64)> {
    match order {
        0 => Ok((1.0, 1.0)),
        1 => {
            let off = |t: f64| (t * (t - 1.0)).max(0.0);
            let hi = 1.0 + 2.0 * off(certified[0]).max(off(certified[1]));
            let lo = if ext[0] < 1.0 && ext[1] > 0.0 {
                0.5
            } else {
                1.0
            };
            Ok((hi, lo))
        }
        n => {
            let w = thumbweight()?;
            if !(w[0].is_finite() && w[1].is_finite() && w[0] <= w[1]) {
                return Err(SolidError::invalid_input(
                    OP,
                    "the thumbweight is not enclosed over the section",
                ));
            }
            gn_weight_range(n as usize, certified, ext, w)
        }
    }
}

/// A G^n section's weight `W(t) = A(t) B(t)^n C(t)^n` (`A = (1 - t)^(n+1) + t^(n+1)`,
/// `B = 1 - w + w t`, `C = 1 - w t`, `w` the thumbweight in `w`): its largest value over the
/// source's t range `src` and its least over the extension's `ext`, rounded outward. `A`
/// grows with `|t - 1/2|` (`A' = (n + 1) (t^n - (1 - t)^n)`, of the sign of `t - 1/2`), so
/// its extremes on an interval are at the point farthest from and nearest to 1/2; `B` and
/// `C` are bilinear in `(t, w)`, so their extremes on a box are at its corners. A corner of
/// the extension's box where `B` or `C` is not positive is a weight that vanishes there:
/// refused ([`ExtendRefusal::WeightVanishes`]).
fn gn_weight_range(n: usize, src: [f64; 2], ext: [f64; 2], w: [f64; 2]) -> Result<(f64, f64)> {
    let exp = i32::try_from(n).map_err(|_| SolidError::sm_err(OP))?;
    let a = |t: f64| (1.0 - t).powi(exp + 1) + t.powi(exp + 1);
    let far = |[x, y]: [f64; 2]| {
        if (x - 0.5).abs() >= (y - 0.5).abs() {
            x
        } else {
            y
        }
    };
    let near = |[x, y]: [f64; 2]| {
        if x <= 0.5 && y >= 0.5 {
            0.5
        } else if (x - 0.5).abs() <= (y - 0.5).abs() {
            x
        } else {
            y
        }
    };
    let corners = |[t0, t1]: [f64; 2]| {
        let mut b = [0.0_f64; 4];
        let mut c = [0.0_f64; 4];
        for (i, (&t, &wt)) in [t0, t0, t1, t1]
            .iter()
            .zip(&[w[0], w[1], w[0], w[1]])
            .enumerate()
        {
            if let (Some(bi), Some(ci)) = (b.get_mut(i), c.get_mut(i)) {
                *bi = 1.0 - wt + wt * t;
                *ci = 1.0 - wt * t;
            }
        }
        (b, c)
    };
    let (bs, cs) = corners(src);
    let mag = |x: &[f64; 4]| x.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    let hi = a(far(src)) * mag(&bs).powi(exp) * mag(&cs).powi(exp);
    let (be, ce) = corners(ext);
    let least = |x: &[f64; 4]| x.iter().fold(f64::INFINITY, |m, &v| m.min(v));
    let (bl, cl) = (least(&be), least(&ce));
    if !(bl > 0.0 && cl > 0.0) {
        return Err(SolidError::ExtendRefused {
            operation: OP,
            refusal: ExtendRefusal::WeightVanishes,
            distance: None,
        });
    }
    let lo = a(near(ext)) * bl.powi(exp) * cl.powi(exp);
    // each a product of 3 n + 2 roundings at most: up and down by gamma_(4 n + 8)
    let g = gamma(4 * n + 8);
    let (hi, lo) = (hi * (1.0 + g), lo * (1.0 - g));
    if !(hi.is_finite() && lo > 0.0) {
        return Err(SolidError::sm_err(OP));
    }
    Ok((hi, lo))
}

/// `T_m(x)` for `x >= 1` (`|x|` taken), by its recurrence (every term positive and
/// increasing there), rounded up.
fn chebyshev(m: usize, x: f64) -> f64 {
    let x = x.abs().max(1.0);
    let (mut prev, mut cur) = (1.0_f64, x);
    if m == 0 {
        return 1.0;
    }
    for _ in 1..m {
        let next = 2.0 * x * cur - prev;
        prev = cur;
        cur = next;
    }
    cur * (1.0 + gamma(3 * m + 3))
}

/// The largest and least control weight of a net (1 for a polynomial one): the `V` of
/// [`growth`].
pub(crate) fn net_weights(net: &Net) -> (f64, f64) {
    let (mut hi, mut lo) = (0.0_f64, f64::INFINITY);
    for c in net.cpts().iter().flatten() {
        let w = if c.w == NOW { 1.0 } else { c.w };
        hi = hi.max(w);
        lo = lo.min(w);
    }
    (hi, lo)
}
