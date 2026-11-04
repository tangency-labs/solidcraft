//! Whether a NURBS surface or curve lies on a sphere, certified on its control points: the
//! certificate a ball patch's net and loop are read with (#2357; the SolidCraft fork's
//! `SmBallPatch::IsOnSphere`, efb8007, whose tolerance this does not keep).
//!
//! A rational B-spline with homogeneous control points `(H, W)` lies on the sphere of centre
//! `C` and radius `R` exactly when `F = |H - C W|^2 - R^2 W^2` vanishes identically, `F` being
//! a polynomial spline of twice the degree in each parameter. Cut into Bezier pieces (each
//! interior knot raised to the degree), `F` on each piece is a Bernstein polynomial whose
//! coefficients are sums of products of the piece's control points; it vanishes exactly when
//! every coefficient does. So the question is decided on the control points, never at sample
//! points, and works on the homogeneous coordinates as stored (no division by a weight).
//!
//! Each coefficient `f` is accepted as zero when both it and its bound are finite and
//! `|f| <= rel_tol (R max W)^2 + e`:
//! - `rel_tol (R max W)^2` is the geometric tolerance, at the sphere's own size, the same
//!   wherever the sphere is (the fork's `rel_tol ((R + |C|) max W)^2` grows with the centre's
//!   distance from the origin, and at `|C| = 1e5`, `R = 3` admits a net millimetres off it);
//! - `e` is a forward error bound of computing `f` in floating point, in Higham's standard
//!   model ([`gamma`]): the cut's knot insertions (convex combinations, `gamma(6)` of the
//!   largest coordinate each), `h = H - C W` at the magnitude of `H` and `C W`, the products
//!   and their sums, the binomial factors and the division. It is the rounding at the
//!   magnitude of the points and the centre, below which the coefficient cannot be decided.
//!
//! A NaN or an infinity anywhere (a hostile record, or a coefficient that overflows) is never
//! accepted: the test is written to accept, so a comparison with NaN refuses. What cannot be
//! decided is refused with a typed error naming the coefficient, its value and its bound.
//!
//! Not upstream. No recursion.
#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::collapse::gamma;
use super::geometry::{Curve, Surface};
use super::spline::{basis_check, MAX_EVAL_POINTS};
use super::EvalError;

/// `IsOnSphere`'s relative tolerance (the fork's `kBallPatchRelTol`, bc814a5): a coefficient
/// is zero to this much of the sphere's size squared, `(R max W)^2`, past its rounding.
pub const ON_SPHERE_REL_TOL: f64 = 1e-12;

/// A surface's homogeneous control points, `grid[i][j]` with `i` along u.
type Grid = Vec<Vec<[f64; 4]>>;

fn invalid(what: &str) -> EvalError {
    EvalError::Invalid(format!("on-sphere certificate: {what}"))
}

/// `n` choose `k` as a double, by its product (exact for the degrees taken).
#[allow(clippy::cast_precision_loss)] // degrees are at most MAX_SPLINE_DEGREE
fn binomial(n: usize, k: usize) -> f64 {
    let mut d = 1.0;
    for i in 1..=k {
        d = d * (n - k + i) as f64 / i as f64;
    }
    d
}

/// The largest |x|, |y|, |z| and the largest |w| of `pts`.
fn magnitudes<'a>(pts: impl Iterator<Item = &'a [f64; 4]>) -> (f64, f64) {
    let (mut mx, mut mw) = (0.0_f64, 0.0_f64);
    for h in pts {
        mx = mx.max(h[0].abs()).max(h[1].abs()).max(h[2].abs());
        mw = mw.max(h[3].abs());
    }
    (mx, mw)
}

/// The knots to insert to cut a clamped spline of degree `p` into Bezier pieces: each interior
/// knot repeated until its multiplicity is `p`.
fn cuts(knots: &[f64], p: usize) -> Result<Vec<f64>, EvalError> {
    let n = knots.len();
    let (Some(first), Some(last)) = (knots.first(), knots.last()) else {
        return Err(invalid("no knots"));
    };
    if !knots.iter().all(|k| k.is_finite()) || knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(invalid("knots are finite and non-decreasing"));
    }
    let clamped = knots.iter().take(p + 1).all(|k| k == first)
        && knots.iter().skip(n - (p + 1)).all(|k| k == last)
        && first < last;
    if !clamped {
        return Err(EvalError::Unsupported(
            "on-sphere certificate: a spline with unclamped knots".into(),
        ));
    }
    let mut add = Vec::new();
    let interior = knots.get(p + 1..n - (p + 1)).unwrap_or(&[]);
    let mut rest = interior;
    while let Some(&t) = rest.first() {
        let mult = rest.iter().take_while(|&&k| k == t).count();
        add.extend(std::iter::repeat_n(t, p.saturating_sub(mult)));
        rest = rest.get(mult..).unwrap_or(&[]);
    }
    Ok(add)
}

/// Inserts `t` once into the spline of degree `p` with `knots` and control points `pts`
/// (Boehm). `t` is an interior knot value of the clamped vector.
fn insert(
    knots: &mut Vec<f64>,
    pts: &mut Vec<[f64; 4]>,
    p: usize,
    t: f64,
) -> Result<(), EvalError> {
    // k: the last knot index with knots[k] <= t; s: t's multiplicity
    let k = knots
        .iter()
        .rposition(|&u| u <= t)
        .ok_or_else(|| invalid("a knot off the range"))?;
    let s = knots.iter().filter(|&&u| u == t).count();
    if k < p || k + 1 >= knots.len() || s >= p {
        return Err(invalid("a knot off the range"));
    }
    let mut out = Vec::with_capacity(pts.len() + 1);
    for i in 0..=pts.len() {
        let q = if i + p <= k {
            pts.get(i).copied()
        } else if i + s <= k {
            let (Some(&a), Some(&b), Some(&ui), Some(&uip)) =
                (pts.get(i - 1), pts.get(i), knots.get(i), knots.get(i + p))
            else {
                return Err(invalid("knots and points do not fit"));
            };
            let den = uip - ui;
            if !(den > 0.0) {
                return Err(invalid("an empty knot span where one is needed"));
            }
            let al = (t - ui) / den;
            let mut q = [0.0; 4];
            for ((qc, bc), ac) in q.iter_mut().zip(b).zip(a) {
                *qc = al * bc + (1.0 - al) * ac;
            }
            Some(q)
        } else {
            pts.get(i - 1).copied()
        };
        out.push(q.ok_or_else(|| invalid("knots and points do not fit"))?);
    }
    knots.insert(k + 1, t);
    *pts = out;
    Ok(())
}

/// The degree of a spline of `count` control points over `knots`.
fn degree(knots: &[f64], count: usize) -> Result<usize, EvalError> {
    let p = knots
        .len()
        .checked_sub(count + 1)
        .ok_or_else(|| invalid("knots and points do not fit"))?;
    if !basis_check(p, count, knots) {
        return Err(invalid(
            "a degree from 1 to 16 and points + degree + 1 knots",
        ));
    }
    Ok(p)
}

/// The control points' check: finite, and positive weights.
fn check_points<'a>(mut pts: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), EvalError> {
    if pts.any(|h| !h.iter().all(|c| c.is_finite()) || !(h[3] > 0.0)) {
        return Err(EvalError::Unevaluable(
            "on-sphere certificate: a control point is not finite or a weight not positive".into(),
        ));
    }
    Ok(())
}

/// One Bernstein coefficient of `|H - C W|^2 - R^2 W^2` on a piece, summed term by term with
/// the bound of its rounding.
struct Coefficient {
    f: f64,
    err: f64,
    mag: f64,
    terms: usize,
}

/// What a coefficient's terms are read with: the sphere and the inputs' rounding.
struct Sphere {
    c: [f64; 3],
    r2: f64,
    ex: f64,
    ew: f64,
}

impl Coefficient {
    const fn new() -> Self {
        Coefficient {
            f: 0.0,
            err: 0.0,
            mag: 0.0,
            terms: 0,
        }
    }

    /// Adds `b (h1 . h2 - R^2 W1 W2)`, `h = H - C W`.
    fn add(&mut self, b: f64, a: &[f64; 4], q: &[f64; 4], s: &Sphere) {
        let (w1, w2) = (a[3], q[3]);
        let mut dot = 0.0;
        let (mut e, mut m) = (0.0, 0.0);
        for ((&ac, &qc), &cc) in a.iter().zip(q).zip(&s.c) {
            let h1 = ac - cc * w1;
            let h2 = qc - cc * w2;
            // each h: the inputs' errors through it, and its own two roundings
            let d1 = s.ex + cc.abs() * s.ew + gamma(2) * (ac.abs() + (cc * w1).abs());
            let d2 = s.ex + cc.abs() * s.ew + gamma(2) * (qc.abs() + (cc * w2).abs());
            dot += h1 * h2;
            e += d1 * h2.abs() + h1.abs() * d2 + d1 * d2;
            m += (h1 * h2).abs();
        }
        let ww = s.r2 * w1 * w2;
        e += s.r2 * (s.ew * w2.abs() + w1.abs() * s.ew + s.ew * s.ew);
        m += ww.abs();
        self.f += b * (dot - ww);
        self.err += b * e;
        self.mag += b * m;
        self.terms += 1;
    }

    /// The coefficient (the sum over `divisor`) and its bound; `degree` the largest degree
    /// whose binomials made the terms and the divisor.
    fn finish(&self, divisor: f64, degree: usize) -> (f64, f64) {
        // the dot product (3 products, 2 sums), R^2 W1 W2 and the difference, the binomials
        // (two products of two per direction, `degree` operations each), the sum, the division
        let k = 8 + 8 * degree + self.terms;
        let bound = (self.err + gamma(k) * self.mag) / divisor * (1.0 + gamma(2));
        (self.f / divisor, bound)
    }
}

/// Whether `f` is decided zero: both finite and `|f| <= bound`.
fn within(f: f64, bound: f64) -> bool {
    f.is_finite() && bound.is_finite() && f.abs() <= bound
}

fn off(what: &str, f: f64, bound: f64) -> EvalError {
    EvalError::Invalid(format!(
        "on-sphere certificate: {what} is {f:e} off the sphere, past its bound {bound:e}"
    ))
}

/// The sphere's check of its own numbers.
fn sphere(centre: [f64; 3], radius: f64) -> Result<(), EvalError> {
    if !(centre.iter().all(|c| c.is_finite()) && radius.is_finite() && radius > 0.0) {
        return Err(invalid(
            "a sphere has a finite centre and a finite, positive radius",
        ));
    }
    Ok(())
}

/// Certifies that the NURBS surface `s` (a `Spline` or `RationalSpline`) lies on the sphere of
/// `centre` and `radius`, every coefficient within `rel_tol (R max W)^2` past its rounding.
///
/// # Errors
///
/// `Invalid` naming the first coefficient off the sphere (its value and bound), or for a sphere
/// or knots that are not well formed; `Unevaluable` for a control point that is not finite or
/// a weight not positive; `Unsupported` for another kind of surface or unclamped knots;
/// `TooLarge` past the evaluator's control point limit.
pub fn surface_on_sphere(
    s: &Surface,
    centre: [f64; 3],
    radius: f64,
    rel_tol: f64,
) -> Result<(), EvalError> {
    sphere(centre, radius)?;
    let (u_knots, v_knots, grid): (&[f64], &[f64], Grid) = match s {
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => (
            u_knots,
            v_knots,
            points
                .iter()
                .map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect())
                .collect(),
        ),
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => (u_knots, v_knots, homogeneous.clone()),
        _ => {
            return Err(EvalError::Unsupported(
                "on-sphere certificate: a surface that is not a NURBS".into(),
            ))
        }
    };
    let nu = grid.len();
    let nv = grid.first().map_or(0, Vec::len);
    if grid.iter().any(|r| r.len() != nv) {
        return Err(invalid("rows of one length"));
    }
    if nu.saturating_mul(nv) > MAX_EVAL_POINTS {
        return Err(EvalError::TooLarge(
            "on-sphere certificate: control points".into(),
        ));
    }
    check_points(grid.iter().flatten())?;
    let (p, q) = (degree(u_knots, nu)?, degree(v_knots, nv)?);
    let (add_u, add_v) = (cuts(u_knots, p)?, cuts(v_knots, q)?);
    if (nu + add_u.len()).saturating_mul(nv + add_v.len()) > MAX_EVAL_POINTS {
        return Err(EvalError::TooLarge(
            "on-sphere certificate: Bezier pieces".into(),
        ));
    }
    let (mx, mw) = magnitudes(grid.iter().flatten());
    // columns (along u) first, then rows (along v)
    let mut cols: Vec<Vec<[f64; 4]>> = (0..nv)
        .map(|j| grid.iter().filter_map(|r| r.get(j).copied()).collect())
        .collect();
    for col in &mut cols {
        let mut knots = u_knots.to_vec();
        for &t in &add_u {
            insert(&mut knots, col, p, t)?;
        }
    }
    let lu = nu + add_u.len();
    let mut rows: Vec<Vec<[f64; 4]>> = (0..lu)
        .map(|i| cols.iter().filter_map(|c| c.get(i).copied()).collect())
        .collect();
    for row in &mut rows {
        let mut knots = v_knots.to_vec();
        for &t in &add_v {
            insert(&mut knots, row, q, t)?;
        }
    }
    let lv = nv + add_v.len();
    // each insertion a convex combination: gamma(6) of the largest coordinate (which a convex
    // combination does not grow), its weight computed from the knots too
    let passes = add_u.len() + add_v.len();
    let w_max = magnitudes(rows.iter().flatten()).1;
    let geometric = rel_tol * (radius * w_max) * (radius * w_max);
    let s = Sphere {
        c: centre,
        r2: radius * radius,
        ex: gamma(6 * passes) * mx,
        ew: gamma(6 * passes) * mw,
    };
    let at = |i: usize, j: usize| -> Result<&[f64; 4], EvalError> {
        rows.get(i)
            .and_then(|r| r.get(j))
            .ok_or_else(|| invalid("the cut is not whole Bezier pieces"))
    };
    if (lu - 1) % p != 0 || (lv - 1) % q != 0 {
        return Err(invalid("the cut is not whole Bezier pieces"));
    }
    let mut bu = 0;
    while bu + 1 < lu {
        let mut bv = 0;
        while bv + 1 < lv {
            for i in 0..=2 * p {
                for j in 0..=2 * q {
                    let mut f = Coefficient::new();
                    for a in i.saturating_sub(p)..=i.min(p) {
                        for b in j.saturating_sub(q)..=j.min(q) {
                            let c = binomial(p, a)
                                * binomial(p, i - a)
                                * binomial(q, b)
                                * binomial(q, j - b);
                            f.add(c, at(bu + a, bv + b)?, at(bu + i - a, bv + j - b)?, &s);
                        }
                    }
                    let (f, e) = f.finish(binomial(2 * p, i) * binomial(2 * q, j), 2 * p.max(q));
                    if !within(f, geometric + e) {
                        return Err(off(
                            &format!(
                                "the net's coefficient ({i}, {j}) of piece ({}, {})",
                                bu / p,
                                bv / q
                            ),
                            f,
                            geometric + e,
                        ));
                    }
                }
            }
            bv += q;
        }
        bu += p;
    }
    Ok(())
}

/// Certifies that the NURBS curve `c` (a `Spline` or `RationalSpline`) lies on the sphere of
/// `centre` and `radius`, as [`surface_on_sphere`] does a surface.
///
/// # Errors
///
/// As [`surface_on_sphere`]'s; `Unsupported` for a curve that is not a NURBS (an analytic
/// curve is read by its NURBS form).
pub fn curve_on_sphere(
    c: &Curve,
    centre: [f64; 3],
    radius: f64,
    rel_tol: f64,
) -> Result<(), EvalError> {
    sphere(centre, radius)?;
    let (knots, mut pts): (&[f64], Vec<[f64; 4]>) = match c {
        Curve::Spline { knots, points } => (
            knots,
            points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        ),
        Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
        _ => {
            return Err(EvalError::Unsupported(
                "on-sphere certificate: a curve that is not a NURBS".into(),
            ))
        }
    };
    if pts.len() > MAX_EVAL_POINTS {
        return Err(EvalError::TooLarge(
            "on-sphere certificate: control points".into(),
        ));
    }
    check_points(pts.iter())?;
    let p = degree(knots, pts.len())?;
    let add = cuts(knots, p)?;
    if pts.len() + add.len() > MAX_EVAL_POINTS {
        return Err(EvalError::TooLarge(
            "on-sphere certificate: Bezier pieces".into(),
        ));
    }
    let (mx, mw) = magnitudes(pts.iter());
    let mut k = knots.to_vec();
    for &t in &add {
        insert(&mut k, &mut pts, p, t)?;
    }
    let n = pts.len();
    if (n - 1) % p != 0 {
        return Err(invalid("the cut is not whole Bezier pieces"));
    }
    let s = Sphere {
        c: centre,
        r2: radius * radius,
        ex: gamma(6 * add.len()) * mx,
        ew: gamma(6 * add.len()) * mw,
    };
    let w_max = magnitudes(pts.iter()).1;
    let geometric = rel_tol * (radius * w_max) * (radius * w_max);
    let at = |i: usize| {
        pts.get(i)
            .ok_or_else(|| invalid("the cut is not whole Bezier pieces"))
    };
    let mut b = 0;
    while b + 1 < n {
        for i in 0..=2 * p {
            let mut f = Coefficient::new();
            for a in i.saturating_sub(p)..=i.min(p) {
                f.add(
                    binomial(p, a) * binomial(p, i - a),
                    at(b + a)?,
                    at(b + i - a)?,
                    &s,
                );
            }
            let (f, e) = f.finish(binomial(2 * p, i), 2 * p);
            if !within(f, geometric + e) {
                return Err(off(
                    &format!("the curve's coefficient {i} of piece {}", b / p),
                    f,
                    geometric + e,
                ));
            }
        }
        b += p;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter of the unit circle about `o` in the plane z = o.z, rational quadratic.
    fn quarter(o: [f64; 3]) -> Curve {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [o[0] + 1.0, o[1], o[2], 1.0],
                [(o[0] + 1.0) * h, (o[1] + 1.0) * h, o[2] * h, h],
                [o[0], o[1] + 1.0, o[2], 1.0],
            ],
        }
    }

    #[test]
    fn a_great_circle_arc_is_on_its_sphere_near_and_far() {
        for o in [[0.0; 3], [1e5, -7e4, 3e4]] {
            curve_on_sphere(&quarter(o), o, 1.0, ON_SPHERE_REL_TOL).unwrap();
            assert!(curve_on_sphere(&quarter(o), o, 1.0 + 1e-9, ON_SPHERE_REL_TOL).is_err());
        }
    }

    #[test]
    fn hostile_numbers_are_refused() {
        let mut c = quarter([0.0; 3]);
        if let Curve::RationalSpline { homogeneous, .. } = &mut c {
            homogeneous[1][3] = f64::NAN;
        }
        assert!(curve_on_sphere(&c, [0.0; 3], 1.0, ON_SPHERE_REL_TOL).is_err());
        assert!(curve_on_sphere(
            &quarter([0.0; 3]),
            [f64::NAN, 0.0, 0.0],
            1.0,
            ON_SPHERE_REL_TOL
        )
        .is_err());
        assert!(curve_on_sphere(
            &quarter([0.0; 3]),
            [0.0; 3],
            f64::INFINITY,
            ON_SPHERE_REL_TOL
        )
        .is_err());
    }
}
