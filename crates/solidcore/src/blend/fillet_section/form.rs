//! The G^n section's rational Bezier form (its weights and, for each jet, the jet's
//! coefficient in each weighted control point) and the piecewise cubic Hermite curves of
//! the jets along the rails.
//!
//! Fork: `sm_Bern`, `sm_BernMul`, `sm_BernPow`, `sm_GnSectionForm`, `sm_GnForm`,
//! `sm_HermitePoints`, `sm_HermiteKnots` in `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9); see [`super`] for the table.

use crate::math::Vec3;

/// `sm_Bern`: a polynomial's Bernstein coefficients, of degree `len() - 1`.
pub type Bern = Vec<f64>;

/// `sm_BernMul`: the product of two Bernstein polynomials (degree `p + q`), each term
/// `C(p, i) C(q, j) / C(p + q, i + j) a_i b_j` added into `r[i + j]`. An empty factor
/// gives an empty product.
#[must_use]
pub fn bern_mul(a: &[f64], b: &[f64]) -> Bern {
    let (Some(p), Some(q)) = (a.len().checked_sub(1), b.len().checked_sub(1)) else {
        return Vec::new();
    };
    let mut r = vec![0.0; p + q + 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            let mut cp = 1.0;
            let mut cq = 1.0;
            let mut cpq = 1.0;
            for m in 1..=i {
                cp *= (p - i + m) as f64 / m as f64;
            }
            for m in 1..=j {
                cq *= (q - j + m) as f64 / m as f64;
            }
            for m in 1..=(i + j) {
                cpq *= (p + q - i - j + m) as f64 / m as f64;
            }
            let c = cp * cq / cpq;
            if let Some(x) = r.get_mut(i + j) {
                *x += c * ai * bj;
            }
        }
    }
    r
}

/// `sm_BernPow`: `a^n` (the constant 1 for `n = 0`), multiplied up from 1.
#[must_use]
pub fn bern_pow(a: &[f64], n: usize) -> Bern {
    let mut r = vec![1.0];
    for _ in 0..n {
        r = bern_mul(&r, a);
    }
    r
}

/// `sm_GnSectionForm`: the section of order n and thumbweight w over the common
/// denominator: its weights `W_j` and, for each jet `c_ik`, the coefficient `g_ik_j` of
/// `c_ik` in `W_j P_j` (`j = 0..3n+1`).
#[derive(Clone, Debug, PartialEq)]
pub struct GnSectionForm {
    /// `W`: the weights, of degree `3n + 1`.
    pub w: Bern,
    /// `aG[i][k]`: jet k of face i's coefficients.
    pub g: [Vec<Bern>; 2],
}

/// `sm_GnForm`: the form of the order-n section with thumbweight w,
/// `W = ((1-t)^(n+1) + t^(n+1)) (1 - w + w t)^n (1 - w t)^n`,
/// `g_0k = (1-t)^(n+1) (1 - w t)^n (-t)^k (1 - w + w t)^(n-k)`,
/// `g_1k = t^(n+1) (1 - w + w t)^n (t - 1)^k (1 - w t)^(n-k)`.
#[must_use]
pub fn gn_form(n: usize, w: f64) -> GnSectionForm {
    let mut a = vec![0.0; n + 2];
    let mut b = vec![0.0; n + 2];
    if let Some(a0) = a.first_mut() {
        *a0 = 1.0; // (1 - t)^(n+1)
    }
    if let Some(bn) = b.last_mut() {
        *bn = 1.0; // t^(n+1)
    }
    let one_minus_wt = [1.0, 1.0 - w]; // 1 - w t
    let one_minus_w_p_wt = [1.0 - w, 1.0]; // 1 - w + w t
    let minus_t = [0.0, -1.0]; // -t
    let t_minus_1 = [-1.0, 0.0]; // t - 1
    let ab: Bern = a.iter().zip(&b).map(|(x, y)| x + y).collect();
    let weights = bern_mul(
        &bern_mul(&ab, &bern_pow(&one_minus_w_p_wt, n)),
        &bern_pow(&one_minus_wt, n),
    );
    let mut g0 = Vec::with_capacity(n + 1);
    let mut g1 = Vec::with_capacity(n + 1);
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
    GnSectionForm {
        w: weights,
        g: [g0, g1],
    }
}

/// `sm_HermitePoints`: the control points (`3 (N - 1) + 1`) of the piecewise cubic Hermite
/// curve on the stations `s` through the values `v` with the s-derivatives `d`.
#[must_use]
pub fn hermite_points(s: &[f64], v: &[Vec3], d: &[Vec3]) -> Vec<Vec3> {
    let mut cps = Vec::new();
    let rows = s.windows(2).zip(v.windows(2)).zip(d.windows(2));
    for (j, ((sw, vw), dw)) in rows.enumerate() {
        let (&[s0, s1], &[v0, v1], &[d0, d1]) = (sw, vw, dw) else {
            continue;
        };
        let h = s1 - s0;
        if j == 0 {
            cps.push(v0);
        }
        cps.push(v0 + d0 * (h / 3.0));
        cps.push(v1 - d1 * (h / 3.0));
        cps.push(v1);
    }
    cps
}

/// `sm_HermiteKnots`: the knots of those curves, the stations with the end ones four times
/// and the inner ones three times (piecewise Bezier), as (knots, multiplicities).
#[must_use]
pub fn hermite_knots(s: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let last = s.len().saturating_sub(1);
    let mult = (0..s.len())
        .map(|j| if j == 0 || j == last { 4 } else { 3 })
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
            for w in [0.0, 0.0342934, 0.5] {
                let form = gn_form(n, w);
                assert_eq!(form.w.len(), 3 * n + 2);
                // the weights are positive (thumbweights in [0, 1))
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

    #[test]
    fn hermite_points_interpolate_values_and_derivatives() {
        let s = [0.0, 1.0, 3.0];
        let v = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ];
        let d = [
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(1.5, 0.0, 0.0),
        ];
        let cps = hermite_points(&s, &v, &d);
        assert_eq!(cps.len(), 3 * (s.len() - 1) + 1);
        assert_eq!(cps[0], v[0]);
        assert_eq!(cps[1], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(cps[3], v[1]);
        assert_eq!(cps[4], Vec3::new(1.0, 3.0, 0.0));
        assert_eq!(cps[5], Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(cps[6], v[2]);
        let (knots, mult) = hermite_knots(&s);
        assert_eq!(knots, s.to_vec());
        assert_eq!(mult, vec![4, 3, 4]);
    }
}
