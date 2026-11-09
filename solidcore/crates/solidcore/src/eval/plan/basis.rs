//! B-spline basis functions (Piegl & Tiller, *The NURBS Book*, eq. 2.5 and 2.9, with the 0/0 = 0
//! convention), shared by the plan evaluator and SolidCraft's splines.

use super::MAX_SPLINE_DEGREE;

/// Which one-sided limit a basis evaluation takes at a knot. Basis functions live on half-open
/// spans `[uᵢ, uᵢ₊₁)` ([`Side::Right`]); at the right end of a domain the left limit on
/// `(uᵢ, uᵢ₊₁]` ([`Side::Left`]) keeps the closing span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

/// One B-spline basis function of degree `p = knots.len() − 2` given by its local knot vector
/// `[ξ₀ … ξₚ₊₁]`, and its derivatives up to order `n`: element `k` is the `k`-th derivative at
/// `u`. Cox–de Boor recursion (NURBS Book eq. 2.5) with the 0/0 = 0 convention, derivatives by
/// eq. 2.9 applied repeatedly (`dᵏNᵢ,ₚ = p·(dᵏ⁻¹Nᵢ,ₚ₋₁/(uᵢ₊ₚ−uᵢ) − dᵏ⁻¹Nᵢ₊₁,ₚ₋₁/(uᵢ₊ₚ₊₁−uᵢ₊₁))`).
/// A knot vector shorter than two knots gives zeros.
pub fn local_basis(knots: &[f64], u: f64, side: Side, n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n + 1];
    let ders = local_basis_array(knots, u, side, n);
    for (o, d) in out.iter_mut().zip(ders) {
        *o = d;
    }
    out
}

/// The `degree + 1` basis functions of a B-spline with `knots` and `count` control points alive
/// at `t`, with their derivatives up to `order`: the index of the first of them and, for each,
/// its value and derivatives (element `k` the `k`-th). Each comes from its own local knots
/// ([`local_basis`]), on the span used at `t` (the last non-empty span starting at or before it,
/// the domain's last span at its end, from the left).
pub fn basis_derivatives(knots: &[f64], degree: usize, count: usize, t: f64, order: usize) -> Option<(usize, Vec<Vec<f64>>)> {
    if degree == 0 || count <= degree || knots.len() != count + degree + 1 || !t.is_finite() {
        return None;
    }
    let span = super::span_of(knots, degree, count, t);
    let side = if t >= *knots.get(count)? { Side::Left } else { Side::Right };
    let first = span.checked_sub(degree)?;
    let ders = (first..=span).map(|i| Some(local_basis(knots.get(i..=i + degree + 1)?, t, side, order))).collect::<Option<Vec<_>>>()?;
    Some((first, ders))
}

/// [`local_basis`] into a fixed array (no allocation): element `k` is the `k`-th derivative,
/// for `k ≤ min(n, MAX_SPLINE_DEGREE)`, the rest zero.
pub fn local_basis_array(knots: &[f64], u: f64, side: Side, n: usize) -> [f64; MAX_SPLINE_DEGREE + 1] {
    const W: usize = MAX_SPLINE_DEGREE + 1;
    let mut out = [0.0; W];
    let Some(p) = knots.len().checked_sub(2) else { return out };
    if p > MAX_SPLINE_DEGREE || !u.is_finite() {
        return out;
    }
    let kn = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let ratio = |num: f64, den: f64| if den > 0.0 { num / den } else { 0.0 };
    // table[k][j]: k-th derivative of N_{j,q} for the current degree q (j = 0..=p−q), updated
    // in place: k descending (level k reads level k−1 of the previous degree), j ascending
    // (N_{j,q} reads N_{j+1,q−1}, not yet overwritten). Derivatives above p are zero.
    let levels = n.min(p);
    let mut table = [[0.0f64; W]; W];
    for (j, slot) in table[0].iter_mut().enumerate().take(p + 1) {
        let (a, b) = (kn(j), kn(j + 1));
        let inside = match side {
            Side::Right => a <= u && u < b,
            Side::Left => a < u && u <= b,
        };
        *slot = if inside { 1.0 } else { 0.0 };
    }
    for q in 1..=p {
        for k in (0..=levels).rev() {
            for j in 0..=p - q {
                let (d1, d2) = (kn(j + q) - kn(j), kn(j + q + 1) - kn(j + 1));
                let at = |k: usize, j: usize| table.get(k).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
                let v = if k == 0 {
                    ratio(u - kn(j), d1) * at(0, j) + ratio(kn(j + q + 1) - u, d2) * at(0, j + 1)
                } else {
                    q as f64 * (ratio(at(k - 1, j), d1) - ratio(at(k - 1, j + 1), d2))
                };
                if let Some(slot) = table.get_mut(k).and_then(|r| r.get_mut(j)) {
                    *slot = v;
                }
            }
        }
    }
    for (k, o) in out.iter_mut().enumerate().take(levels + 1) {
        *o = table.get(k).map_or(0.0, |r| r[0]);
    }
    out
}

/// The binomial coefficient `(n k)` (zero for `k > n`).
pub fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |c, m| c * (n - m) as f64 / (m + 1) as f64)
}
