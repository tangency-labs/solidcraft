//! The G^n section's rational Bezier form (its weights and, for each jet, the jet's
//! coefficient in each weighted control point) and the piecewise quintic Hermite curves of
//! the jets along the rails.
//!
//! Fork: `sm_Bern`, `sm_Binom`, `sm_BernMul`, `sm_BernPow`, `sm_GnSectionFormT`,
//! `sm_GnFormT`, `sm_GnForm`, `sm_GnFormNormalized`, `sm_HermitePoints`, `sm_HermiteKnots`
//! in `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib@11ea2d9; the ring-generic
//! form, `sm_Binom` and the normalized form 7102bc5); see [`super`] for the table.

use super::series::HERMITE;
use super::tay::Ring;
use crate::math::Vec3;

/// `sm_Bern`: a polynomial's Bernstein coefficients, of degree `len() - 1`.
pub type Bern = Vec<f64>;

/// `sm_Binom`: `C(p, i)`, exact for the degrees here (rounded to the nearest integer).
#[must_use]
pub fn binom(p: usize, i: usize) -> f64 {
    let mut c = 1.0_f64;
    for m in 1..=i {
        c = c * (p - i + m) as f64 / m as f64;
    }
    (c + 0.5).floor()
}

/// `sm_BernMul`: the product of two Bernstein polynomials (degree `p + q`) in any ring Z,
/// each term `C(p, i) C(q, j) / C(p + q, i + j) a_i b_j` added into `r[i + j]`. An empty
/// factor gives an empty product.
#[must_use]
pub fn bern_mul<Z: Ring>(a: &[Z], b: &[Z]) -> Vec<Z> {
    let (Some(p), Some(q)) = (a.len().checked_sub(1), b.len().checked_sub(1)) else {
        return Vec::new();
    };
    let mut r = vec![Z::lift(0.0); p + q + 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            let c = Z::lift(binom(p, i) * binom(q, j)) / Z::lift(binom(p + q, i + j));
            if let Some(x) = r.get_mut(i + j) {
                *x = *x + c * ai * bj;
            }
        }
    }
    r
}

/// `sm_BernPow`: `a^n` (the constant 1 for `n = 0`), multiplied up from 1.
#[must_use]
pub fn bern_pow<Z: Ring>(a: &[Z], n: usize) -> Vec<Z> {
    let mut r = vec![Z::lift(1.0)];
    for _ in 0..n {
        r = bern_mul(&r, a);
    }
    r
}

/// `sm_GnSectionFormT`: the section of order n and thumbweight w over the common
/// denominator, in any ring Z: its weights `W_j` and, for each jet `c_ik`, the coefficient
/// `g_ik_j` of `c_ik` in `W_j P_j` (`j = 0..3n+1`).
#[derive(Clone, Debug, PartialEq)]
pub struct GnSectionFormT<Z> {
    /// `W`: the weights, of degree `3n + 1`.
    pub w: Vec<Z>,
    /// `aG[i][k]`: jet k of face i's coefficients.
    pub g: [Vec<Vec<Z>>; 2],
}

/// `sm_GnSectionForm`: the form in doubles.
pub type GnSectionForm = GnSectionFormT<f64>;

/// `sm_GnFormT`: the form of the order-n section with thumbweight w,
/// `W = ((1-t)^(n+1) + t^(n+1)) (1 - w + w t)^n (1 - w t)^n`,
/// `g_0k = (1-t)^(n+1) (1 - w t)^n (-t)^k (1 - w + w t)^(n-k)`,
/// `g_1k = t^(n+1) (1 - w + w t)^n (t - 1)^k (1 - w t)^(n-k)`.
#[must_use]
pub fn gn_form_t<Z: Ring>(n: usize, w: Z) -> GnSectionFormT<Z> {
    let zero = Z::lift(0.0);
    let one = Z::lift(1.0);
    let mut a = vec![zero; n + 2];
    let mut b = vec![zero; n + 2];
    if let Some(a0) = a.first_mut() {
        *a0 = one; // (1 - t)^(n+1)
    }
    if let Some(bn) = b.last_mut() {
        *bn = one; // t^(n+1)
    }
    let one_minus_wt = [one, one - w]; // 1 - w t
    let one_minus_w_p_wt = [one - w, one]; // 1 - w + w t
    let minus_t = [zero, Z::lift(-1.0)]; // -t
    let t_minus_1 = [Z::lift(-1.0), zero]; // t - 1
    let ab: Vec<Z> = a.iter().zip(&b).map(|(&x, &y)| x + y).collect();
    let weights = bern_mul(
        &bern_mul(&ab, &bern_pow(&one_minus_w_p_wt, n)),
        &bern_pow(&one_minus_wt, n),
    );
    let mut g0 = Vec::new();
    let mut g1 = Vec::new();
    for k in 0..=n {
        g0.push(bern_mul(
            &bern_mul(&a, &bern_pow(&one_minus_wt, n)),
            &bern_mul(&bern_pow(&minus_t, k), &bern_pow(&one_minus_w_p_wt, n - k)),
        ));
        g1.push(bern_mul(
            &bern_mul(&b, &bern_pow(&one_minus_w_p_wt, n)),
            &bern_mul(&bern_pow(&t_minus_1, k), &bern_pow(&one_minus_wt, n - k)),
        ));
    }
    GnSectionFormT {
        w: weights,
        g: [g0, g1],
    }
}

/// `sm_GnForm`: [`gn_form_t`] in doubles.
#[must_use]
pub fn gn_form(n: usize, w: f64) -> GnSectionForm {
    gn_form_t(n, w)
}

/// `sm_GnFormNormalized`: the form over `W_0` (the weight at the first rail, `(1 - w)^n`,
/// as at the second): the same section, its rows a common factor apart, with the weight 1
/// at the first rail whatever w is, so that rows interpolated along s keep that rail the
/// jets' own curve.
#[must_use]
pub fn gn_form_normalized<Z: Ring>(n: usize, w: Z) -> GnSectionFormT<Z> {
    let mut form = gn_form_t(n, w);
    let Some(&w0) = form.w.first() else {
        return form;
    };
    for j in 0..form.w.len() {
        if let Some(x) = form.w.get_mut(j) {
            *x = *x / w0;
        }
        for gi in &mut form.g {
            for gk in gi.iter_mut() {
                if let Some(x) = gk.get_mut(j) {
                    *x = *x / w0;
                }
            }
        }
    }
    form
}

/// The s-derivatives a station gives a Hermite span at each end, `HERMITE / 2 - 1`
/// (values, first and second derivatives).
pub const JET_ORDER: usize = HERMITE / 2 - 1;

/// A station's jet as a Hermite curve takes it: its value, and its first and second
/// s-derivatives as the span starting there takes them (`out`) and as the span ending there
/// does (`into`): a station on a face's knot line has one of each (3bc54a4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HermiteJet {
    /// The value.
    pub value: Vec3,
    /// `[d/ds, d^2/ds^2]` on the pieces after the station.
    pub out: [Vec3; JET_ORDER],
    /// `[d/ds, d^2/ds^2]` on the pieces before it.
    pub into: [Vec3; JET_ORDER],
}

/// The degree of the Hermite curves along s, `HERMITE - 1` (quintic).
pub const HERMITE_DEGREE: usize = HERMITE - 1;

// [`hermite_points_sided`] is the quintic one: values, first and second derivatives
const _: () = assert!(HERMITE == 6);

/// `sm_HermitePoints`, quintic (not the fork's: its curves are cubic, through values and
/// first derivatives): the control points (`HERMITE_DEGREE (N - 1) + 1`) of the piecewise
/// quintic Hermite curve on the stations `s` through the jets `jets`, each span the quintic
/// with its ends' values and first and second s-derivatives (the span's own sides of them,
/// [`HermiteJet`]). On a span of length h from `(v0, d0, a0)` to `(v1, d1, a1)` its Bezier
/// points are `v0`, `v0 + h d0 / 5`, `v0 + 2 h d0 / 5 + h^2 a0 / 20`,
/// `v1 - 2 h d1 / 5 + h^2 a1 / 20`, `v1 - h d1 / 5`, `v1`.
#[must_use]
pub fn hermite_points_sided(s: &[f64], jets: &[HermiteJet]) -> Vec<Vec3> {
    let mut cps = Vec::new();
    for (j, (sw, jw)) in s.windows(2).zip(jets.windows(2)).enumerate() {
        let (&[s0, s1], [a, b]) = (sw, jw) else {
            continue;
        };
        let h = s1 - s0;
        let [d0, a0] = a.out;
        let [d1, a1] = b.into;
        if j == 0 {
            cps.push(a.value);
        }
        cps.push(a.value + d0 * (h / 5.0));
        cps.push(a.value + d0 * (2.0 * h / 5.0) + a0 * (h * h / 20.0));
        cps.push(b.value - d1 * (2.0 * h / 5.0) + a1 * (h * h / 20.0));
        cps.push(b.value - d1 * (h / 5.0));
        cps.push(b.value);
    }
    cps
}

/// `sm_HermiteKnots`: the knots of those curves, the stations with the end ones
/// `HERMITE_DEGREE + 1` times and the inner ones `HERMITE_DEGREE` times (piecewise Bezier),
/// as (knots, multiplicities).
#[must_use]
pub fn hermite_knots(s: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let last = s.len().saturating_sub(1);
    let mult = (0..s.len())
        .map(|j| {
            if j == 0 || j == last {
                HERMITE_DEGREE + 1
            } else {
                HERMITE_DEGREE
            }
        })
        .collect();
    (s.to_vec(), mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Bernstein polynomial's value at t (de Casteljau).
    fn eval(b: &[f64], t: f64) -> f64 {
        let mut c = b.to_vec();
        for r in 1..c.len() {
            for i in 0..c.len() - r {
                c[i] = (1.0 - t) * c[i] + t * c[i + 1];
            }
        }
        c[0]
    }

    #[test]
    fn product_and_power_are_the_polynomials() {
        // (1 - t + 2 t) (3 (1-t)^2 + 2 (1-t) t + 5 t^2)
        let a = [1.0, 2.0];
        let b = [3.0, 1.0, 5.0];
        let ab = bern_mul(&a, &b);
        assert_eq!(ab.len(), 4);
        for t in [0.0, 0.25, 0.6, 1.0] {
            assert!((eval(&ab, t) - eval(&a, t) * eval(&b, t)).abs() < 1e-14);
        }
        let p = bern_pow(&a, 3);
        for t in [0.0, 0.3, 1.0] {
            assert!((eval(&p, t) - eval(&a, t).powi(3)).abs() < 1e-13);
        }
        assert_eq!(bern_pow(&a, 0), vec![1.0]);
        assert!(bern_mul(&[], &a).is_empty());
    }

    /// The form is the section: `sum_k g_ik(t) / W(t)` is `f (-p1)^k` for face 0 and
    /// `(1 - f) (p2 - 1)^k` for face 1, as the module's definition of S has it.
    #[test]
    fn form_is_the_section_of_its_definition() {
        for n in 2..=4_usize {
            for w in [-0.9, -0.5252124, 0.0, 0.0342934, 0.5] {
                let form = gn_form(n, w);
                assert_eq!(form.w.len(), 3 * n + 2);
                // the weights are positive (thumbweights below 1)
                assert!(form.w.iter().all(|&x| x > 0.0));
                for t in [0.1_f64, 0.4, 0.5, 0.9] {
                    let a = (1.0 - t).powi(n as i32 + 1);
                    let b = t.powi(n as i32 + 1);
                    let f = a / (a + b);
                    let p1 = t / (1.0 - w + w * t);
                    let p2 = t * (1.0 - w) / (1.0 - w * t);
                    let ww = eval(&form.w, t);
                    for k in 0..=n {
                        let g0 = eval(&form.g[0][k], t) / ww;
                        let g1 = eval(&form.g[1][k], t) / ww;
                        // T1(1 - p1) has (u - 1)^k = (-p1)^k; T2(p2) has (p2 - 1)^k
                        assert!(
                            (g0 - f * (-p1).powi(k as i32)).abs() < 1e-13,
                            "{n} {w} {t} {k}"
                        );
                        assert!(
                            (g1 - (1.0 - f) * (p2 - 1.0).powi(k as i32)).abs() < 1e-13,
                            "{n} {w} {t} {k}"
                        );
                    }
                }
            }
        }
    }

    /// A quintic's jets at the stations, its value and first and second derivatives.
    fn quintic_jet(s: f64) -> HermiteJet {
        // x = s^5 - 2 s^3 + s, y = 3 s^4 - s^2 + 2, z = 0.5 s^5 + s^4
        let v = Vec3::new(
            s.powi(5) - 2.0 * s.powi(3) + s,
            3.0 * s.powi(4) - s * s + 2.0,
            0.5 * s.powi(5) + s.powi(4),
        );
        let d = Vec3::new(
            5.0 * s.powi(4) - 6.0 * s * s + 1.0,
            12.0 * s.powi(3) - 2.0 * s,
            2.5 * s.powi(4) + 4.0 * s.powi(3),
        );
        let a = Vec3::new(
            20.0 * s.powi(3) - 12.0 * s,
            36.0 * s * s - 2.0,
            10.0 * s.powi(3) + 12.0 * s * s,
        );
        HermiteJet {
            value: v,
            out: [d, a],
            into: [d, a],
        }
    }

    /// A Bezier curve's point at t (de Casteljau).
    fn bezier_at(p: &[Vec3], t: f64) -> Vec3 {
        let mut c = p.to_vec();
        for r in 1..c.len() {
            for i in 0..c.len() - r {
                c[i] = c[i] * (1.0 - t) + c[i + 1] * t;
            }
        }
        c[0]
    }

    #[test]
    fn hermite_points_sided_are_the_quintic_through_values_and_two_derivatives() {
        let s = [0.0, 0.5, 2.0];
        let jets: Vec<HermiteJet> = s.iter().map(|&x| quintic_jet(x)).collect();
        let cps = hermite_points_sided(&s, &jets);
        assert_eq!(cps.len(), HERMITE_DEGREE * (s.len() - 1) + 1);
        // each span reproduces the quintic its jets are of
        for (j, w) in s.windows(2).enumerate() {
            let piece = &cps[HERMITE_DEGREE * j..=HERMITE_DEGREE * (j + 1)];
            for t in [0.0, 0.2, 0.5, 0.77, 1.0] {
                let x = w[0] + t * (w[1] - w[0]);
                let want = quintic_jet(x).value;
                let got = bezier_at(piece, t);
                assert!((got - want).length() < 1e-12, "{j} {t}: {got:?} {want:?}");
            }
        }
        // the derivatives the span ending at the middle takes move only its last points
        // before the station; those the span starting there takes, its first after it
        let mut kinked = jets.clone();
        kinked[1].into = [Vec3::new(6.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let k = hermite_points_sided(&s, &kinked);
        for i in (0..cps.len()).filter(|&i| i != 3 && i != 4) {
            assert_eq!(k[i], cps[i], "{i}");
        }
        assert_ne!(k[3], cps[3]);
        assert_ne!(k[4], cps[4]);
        let mut kinked = jets.clone();
        kinked[1].out = [Vec3::new(6.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let k = hermite_points_sided(&s, &kinked);
        for i in (0..cps.len()).filter(|&i| i != 6 && i != 7) {
            assert_eq!(k[i], cps[i], "{i}");
        }
        let (knots, mult) = hermite_knots(&s);
        assert_eq!(knots, s.to_vec());
        assert_eq!(mult, vec![6, 5, 6]);
    }
}
