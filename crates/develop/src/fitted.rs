//! A free-form panel's flattening, fitted in its own surface's spline space.
//!
//! The flat map F(u, v) is a planar spline over the panel's surface's own knots, refined where it
//! must be. The surface's parameter lines therefore carry to the flat exactly, and so does
//! anything drawn in its parameters. F is fitted to the neutral surface's metric, its first
//! fundamental form I - 2hII + h^2 III, with h the depth of the neutral surface below the skin,
//! rather than to positions: a developable surface has an F whose metric matches it everywhere,
//! and on any other surface what is left is the stretch the flat costs, which is reported and
//! never forced. A grid on the neutral surface unfolded triangle by triangle is the starting
//! guess.

use solidcraft_geom::bspline::{NurbsSurface, bernstein};
use solidcraft_geom::spline_space::{Space, embed};
use solidcraft_geom::{Seg2, Vec2, Vec3};

/// The stretch a fitted flat may leave, as a strain: refinement stops below it.
pub(crate) const STRETCH_TOL: f64 = 1e-7;

/// The most unknowns one fit solves for (two per control point): a work limit for the dense
/// solve.
const MAX_UNKNOWNS: usize = 1600;

/// How many times the fit's space is refined at most.
const MAX_REFINEMENTS: usize = 7;

/// Damped Gauss-Newton steps per fit.
const MAX_STEPS: usize = 40;

/// A spline surface's point and first and second derivatives at `(u, v)`: S, Su, Sv, Suu, Suv,
/// Svv.
pub(crate) fn derivatives(s: &NurbsSurface, spaces: &[Space; 2], u: f64, v: f64) -> Option<[Vec3; 6]> {
    let [su, sv] = spaces;
    let [nu, _] = s.count();
    let bu: Vec<(usize, Vec<f64>)> = (0..3).map(|d| su.basis(u, d)).collect::<Result<_, _>>().ok()?;
    let bv: Vec<(usize, Vec<f64>)> = (0..3).map(|d| sv.basis(v, d)).collect::<Result<_, _>>().ok()?;
    // The homogeneous sum for each pair of derivative orders.
    let sum = |a: usize, b: usize| -> Option<(Vec3, f64)> {
        let ((iu, wu), (iv, wv)) = (bu.get(a)?, bv.get(b)?);
        let (mut p, mut w) = (Vec3::ZERO, 0.0);
        for (j, bj) in wv.iter().enumerate() {
            for (i, bi) in wu.iter().enumerate() {
                let c = s.homogeneous().get((iv + j) * nu + iu + i)?;
                let k = bi * bj;
                p += c.xyz() * k;
                w += c.w * k;
            }
        }
        Some((p, w))
    };
    let (a, w) = sum(0, 0)?;
    let (au, wu) = sum(1, 0)?;
    let (av, wv) = sum(0, 1)?;
    let (auu, wuu) = sum(2, 0)?;
    let (auv, wuv) = sum(1, 1)?;
    let (avv, wvv) = sum(0, 2)?;
    if !(w.abs() > f64::MIN_POSITIVE) {
        return None;
    }
    // The quotient rule up to second order (Piegl and Tiller, eq. 4.20).
    let p = a * (1.0 / w);
    let pu = (au - p * wu) * (1.0 / w);
    let pv = (av - p * wv) * (1.0 / w);
    let puu = (auu - pu * (2.0 * wu) - p * wuu) * (1.0 / w);
    let puv = (auv - pu * wv - pv * wu - p * wuv) * (1.0 / w);
    let pvv = (avv - pv * (2.0 * wv) - p * wvv) * (1.0 / w);
    Some([p, pu, pv, puu, puv, pvv])
}

/// The neutral surface's unit normal toward the material, depth below the skin, and first
/// fundamental form (E, F, G) at a point whose derivatives are `d`. The material lies on the
/// side `Su x Sv` points to; the neutral surface lies K x T from the inside of the bend there,
/// which is the material side where the surface curves toward it.
fn neutral(d: &[Vec3; 6], thickness: f64, k_factor: f64) -> Option<(Vec3, f64, [f64; 3])> {
    let [_, su, sv, suu, suv, svv] = *d;
    let n = su.cross(sv).normalized()?;
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
    let det = e * g - f * f;
    if !(det > 0.0) {
        return None;
    }
    let mean = (e * nn - 2.0 * f * m + g * l) / (2.0 * det);
    let h = if mean > 0.0 { (1.0 - k_factor) * thickness } else { k_factor * thickness };
    // III = II I^-1 II.
    let (i00, i01, i11) = (g / det, -f / det, e / det);
    let (a00, a01, a10, a11) = (l * i00 + m * i01, l * i01 + m * i11, m * i00 + nn * i01, m * i01 + nn * i11);
    let (t00, t01, t11) = (a00 * l + a01 * m, a00 * m + a01 * nn, a10 * m + a11 * nn);
    Some((n, h, [e - 2.0 * h * l + h * h * t00, f - 2.0 * h * m + h * h * t01, g - 2.0 * h * nn + h * h * t11]))
}

/// A planar spline F(u, v) over a tensor space: the flat a free-form panel's parameters map to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlatMap {
    spaces: [Space; 2],
    /// Control points, u fastest.
    ctrl: Vec<Vec2>,
    /// The largest strain left between F's metric and the neutral surface's.
    pub(crate) stretch: f64,
}

impl FlatMap {
    /// F and its first derivatives at `(u, v)`.
    fn eval(&self, u: f64, v: f64) -> Option<[Vec2; 3]> {
        let nu = self.spaces[0].count();
        let ((iu, b0u), (_, b1u)) = (self.spaces[0].basis(u, 0).ok()?, self.spaces[0].basis(u, 1).ok()?);
        let ((iv, b0v), (_, b1v)) = (self.spaces[1].basis(v, 0).ok()?, self.spaces[1].basis(v, 1).ok()?);
        let mut out = [Vec2::ZERO; 3];
        for j in 0..b0v.len() {
            for i in 0..b0u.len() {
                let c = *self.ctrl.get((iv + j) * nu + iu + i)?;
                let (bu, bv, du, dv) = (*b0u.get(i)?, *b0v.get(j)?, *b1u.get(i)?, *b1v.get(j)?);
                out[0] += c * (bu * bv);
                out[1] += c * (du * bv);
                out[2] += c * (bu * dv);
            }
        }
        Some(out)
    }

    pub(crate) fn point(&self, p: Vec2) -> Vec2 {
        self.eval(p.x, p.y).map_or(p, |e| e[0])
    }

    pub(crate) fn dir(&self, p: Vec2, v: Vec2) -> Option<Vec2> {
        let [_, fu, fv] = self.eval(p.x, p.y)?;
        (fu * v.x + fv * v.y).normalized()
    }

    /// A straight segment of the parameters flattened: F along a line is a polynomial of the
    /// spaces' degrees added (those of the directions it moves in) on each piece between knot
    /// lines, so it comes back exactly as Bezier pieces. Higher than a segment can hold, it is
    /// followed by chords.
    pub(crate) fn line(&self, a: Vec2, b: Vec2, tol: f64, out: &mut Vec<Seg2>) {
        let mut cuts = vec![0.0, 1.0];
        for (axis, (x0, x1)) in [(a.x, b.x), (a.y, b.y)].into_iter().enumerate() {
            if (x1 - x0).abs() > 0.0 {
                for &k in self.spaces[axis].knots() {
                    let t = (k - x0) / (x1 - x0);
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-14);
        let degree = (if a.x != b.x { self.spaces[0].degree() } else { 0 }) + (if a.y != b.y { self.spaces[1].degree() } else { 0 });
        let at = |t: f64| self.point(a.lerp(b, t));
        for w in cuts.windows(2) {
            let [t0, t1] = [w[0], w[1]];
            match bezier_through(degree, |s| at(t0 + (t1 - t0) * s)) {
                Some(seg) => out.push(seg),
                None => {
                    let n = ((a.dist(b) * (t1 - t0) / tol).sqrt().ceil() as usize).clamp(2, 4096);
                    let pts: Vec<Vec2> = (0..=n).map(|k| at(t0 + (t1 - t0) * k as f64 / n as f64)).collect();
                    out.extend(pts.windows(2).filter_map(|p| match p {
                        [p, q] => Some(Seg2::Line { a: *p, b: *q }),
                        _ => None,
                    }));
                }
            }
        }
    }
}

/// The Bezier of `degree` through the polynomial curve `f` on [0, 1], as the segment kind that
/// holds it: found from `degree + 1` points, which a polynomial of that degree is fixed by.
fn bezier_through(degree: usize, f: impl Fn(f64) -> Vec2) -> Option<Seg2> {
    if degree == 0 || degree > solidcraft_geom::MAX_BEZIER_DEGREE {
        return None;
    }
    let n = degree + 1;
    let ts: Vec<f64> = (0..n).map(|k| k as f64 / degree as f64).collect();
    let mut m: Vec<f64> = ts.iter().flat_map(|&t| (0..n).map(move |i| bernstein(degree, i, t))).collect();
    let mut rhs: Vec<Vec2> = ts.iter().map(|&t| f(t)).collect();
    solve_dense(&mut m, &mut rhs, n)?;
    Some(match (degree, rhs.as_slice()) {
        (1, [p, q]) => Seg2::Line { a: *p, b: *q },
        (2, [p, c, q]) => Seg2::Conic { a: *p, apex: *c, b: *q, w: 1.0 },
        (3, [p0, p1, p2, p3]) => Seg2::Cubic { p0: *p0, p1: *p1, p2: *p2, p3: *p3 },
        _ => {
            let mut p = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
            for (slot, c) in p.iter_mut().zip(&rhs) {
                *slot = *c;
            }
            Seg2::Bezier { n: u8::try_from(degree).ok()?, p }
        }
    })
}

/// Solves the dense `n x n` system `m x = rhs` in place (Gaussian elimination with partial
/// pivoting), the solution left in `rhs`.
fn solve_dense<T: Copy + std::ops::Sub<Output = T> + std::ops::Mul<f64, Output = T>>(m: &mut [f64], rhs: &mut [T], n: usize) -> Option<()> {
    if m.len() != n * n || rhs.len() != n {
        return None;
    }
    for c in 0..n {
        let pivot = (c..n).max_by(|&a, &b| m[a * n + c].abs().total_cmp(&m[b * n + c].abs()))?;
        if !(m[pivot * n + c].abs() > 1e-300) {
            return None;
        }
        if pivot != c {
            for k in 0..n {
                m.swap(c * n + k, pivot * n + k);
            }
            rhs.swap(c, pivot);
        }
        for r in c + 1..n {
            let f = m[r * n + c] / m[c * n + c];
            if f != 0.0 {
                for k in c..n {
                    m[r * n + k] -= f * m[c * n + k];
                }
                rhs[r] = rhs[r] - rhs[c] * f;
            }
        }
    }
    for c in (0..n).rev() {
        let mut x = rhs[c];
        for k in c + 1..n {
            x = x - rhs[k] * m[c * n + k];
        }
        rhs[c] = x * (1.0 / m[c * n + c]);
    }
    Some(())
}

/// Samples of the neutral surface at which a fit is measured: points inside every knot span of
/// `spaces`, each with its neutral metric.
fn samples(s: &NurbsSurface, s_spaces: &[Space; 2], spaces: &[Space; 2], thickness: f64, k_factor: f64) -> Option<Vec<(Vec2, [f64; 3])>> {
    let along = |sp: &Space| -> Vec<f64> {
        let mut breaks: Vec<f64> = sp.knots().to_vec();
        breaks.dedup();
        let per = sp.degree() + 2;
        breaks.windows(2).flat_map(|w| (0..per).map(move |k| w[0] + (w[1] - w[0]) * (k as f64 + 0.5) / per as f64)).collect()
    };
    let (us, vs) = (along(&spaces[0]), along(&spaces[1]));
    let mut out = Vec::with_capacity(us.len() * vs.len());
    for &v in &vs {
        for &u in &us {
            let d = derivatives(s, s_spaces, u, v)?;
            let (_, _, metric) = neutral(&d, thickness, k_factor)?;
            out.push((Vec2::new(u, v), metric));
        }
    }
    Some(out)
}

/// Fits the flat map of the spline surface `s`'s neutral surface, for sheet `thickness` and
/// `k_factor`.
pub(crate) fn fit(s: &NurbsSurface, thickness: f64, k_factor: f64) -> Result<FlatMap, &'static str> {
    let [p, q] = s.degree();
    let [ku, kv] = s.knots().clone();
    let s_spaces = [
        Space::new(p, ku).map_err(|_| "a free-form panel's surface needs clamped knots")?,
        Space::new(q, kv).map_err(|_| "a free-form panel's surface needs clamped knots")?,
    ];
    // F's own space: over the surface's parameters and breaks, so that its parameter lines and
    // anything drawn in them stay the surface's. Along a straight direction (degree 1, the
    // rulings of a ruled surface) a development is exactly linear, so it stays as it is and is
    // never refined; along a curved one F is cubic, which converges quickly under refinement.
    let curved = [p > 1, q > 1];
    let own = |sp: &Space, curved: bool| -> Option<Space> {
        if !curved {
            return Some(sp.clone());
        }
        let mut breaks: Vec<f64> = sp.knots().to_vec();
        breaks.dedup();
        Space::with_breaks(sp.degree().max(3), &breaks).ok()
    };
    let (Some(fu), Some(fv)) = (own(&s_spaces[0], curved[0]), own(&s_spaces[1], curved[1])) else {
        return Err("a free-form panel's surface has knots its flat cannot be fitted over");
    };
    let mut spaces = [fu, fv];
    // The starting guess: positions from the unfolded grid, fitted in the surface's own space.
    let mut ctrl = seed(s, &s_spaces, &spaces, thickness, k_factor).ok_or("a free-form panel's surface could not be unfolded")?;
    let mut map = FlatMap { spaces: spaces.clone(), ctrl: ctrl.clone(), stretch: f64::INFINITY };
    for _ in 0..=MAX_REFINEMENTS {
        let pts = samples(s, &s_spaces, &spaces, thickness, k_factor).ok_or("a free-form panel's surface has a point with no normal")?;
        ctrl = gauss_newton(&spaces, ctrl, &pts);
        map = FlatMap { spaces: spaces.clone(), ctrl: ctrl.clone(), stretch: strain(&spaces, &ctrl, &pts) };
        if map.stretch <= STRETCH_TOL {
            break;
        }
        // Finer: a knot in the middle of every span of both directions. The surface's own
        // parameters stay F's, so nothing drawn in them moves.
        let finer = |sp: &Space| -> Option<Space> {
            let mut b: Vec<f64> = sp.knots().to_vec();
            b.dedup();
            let mids: Vec<f64> = b.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
            let mut all = b;
            all.extend(mids);
            all.sort_by(f64::total_cmp);
            sp.refined(&all).ok()
        };
        let refine = |sp: &Space, curved: bool| if curved { finer(sp) } else { Some(sp.clone()) };
        let (Some(fu), Some(fv)) = (refine(&spaces[0], curved[0]), refine(&spaces[1], curved[1])) else { break };
        if 2 * fu.count() * fv.count() > MAX_UNKNOWNS || !(curved[0] || curved[1]) {
            break;
        }
        let Some(lifted) = lift(&spaces, &ctrl, &[fu.clone(), fv.clone()]) else { break };
        spaces = [fu, fv];
        ctrl = lifted;
    }
    Ok(map)
}

/// Control points of the same map in a finer tensor space (exact: knot insertion row by row,
/// then column by column).
fn lift(from: &[Space; 2], ctrl: &[Vec2], to: &[Space; 2]) -> Option<Vec<Vec2>> {
    let (nu, nv) = (from[0].count(), from[1].count());
    let mu = to[0].count();
    let as3 = |p: Vec2| Vec3::new(p.x, p.y, 0.0);
    let mut rows = Vec::with_capacity(nv);
    for j in 0..nv {
        let row: Vec<Vec3> = (0..nu).map(|i| ctrl.get(j * nu + i).copied().map(as3)).collect::<Option<_>>()?;
        rows.push(embed(&from[0], &row, &to[0]).ok()?);
    }
    let mut out = vec![Vec2::ZERO; mu * to[1].count()];
    for i in 0..mu {
        let col: Vec<Vec3> = rows.iter().map(|r| r.get(i).copied()).collect::<Option<_>>()?;
        let lifted = embed(&from[1], &col, &to[1]).ok()?;
        for (j, c) in lifted.into_iter().enumerate() {
            *out.get_mut(j * mu + i)? = Vec2::new(c.x, c.y);
        }
    }
    Some(out)
}

/// A grid on the neutral surface unfolded triangle by triangle, then fitted in `spaces` by least
/// squares on positions.
fn seed(s: &NurbsSurface, s_spaces: &[Space; 2], spaces: &[Space; 2], thickness: f64, k_factor: f64) -> Option<Vec<Vec2>> {
    let n = |sp: &Space| (sp.count() * 3).clamp(8, 64);
    let (nu, nv) = (n(&spaces[0]), n(&spaces[1]));
    let [[u0, u1], [v0, v1]] = [spaces[0].domain(), spaces[1].domain()];
    let mut params = Vec::with_capacity(nu * nv);
    let mut pts = Vec::with_capacity(nu * nv);
    let mut normals = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for i in 0..nu {
            let (u, v) = (u0 + (u1 - u0) * i as f64 / (nu - 1) as f64, v0 + (v1 - v0) * j as f64 / (nv - 1) as f64);
            let d = derivatives(s, s_spaces, u, v)?;
            let (normal, h, _) = neutral(&d, thickness, k_factor)?;
            params.push(Vec2::new(u, v));
            pts.push(d[0] + normal * h);
            normals.push(normal);
        }
    }
    let at = |i: usize, j: usize| j * nu + i;
    let mut flat: Vec<Option<Vec2>> = vec![None; nu * nv];
    *flat.get_mut(at(0, 0))? = Some(Vec2::ZERO);
    *flat.get_mut(at(1, 0))? = Some(Vec2::new(pts.get(at(1, 0))?.dist(*pts.get(at(0, 0))?), 0.0));
    // Each new point from two placed ones: the distances from the surface, on the side the
    // material's normal says (the parameters' u x v is the flat's +z).
    let mut place = |x: usize, a: usize, b: usize| -> Option<()> {
        let (fa, fb) = ((*flat.get(a)?)?, (*flat.get(b)?)?);
        let (pa, pb, px) = (*pts.get(a)?, *pts.get(b)?, *pts.get(x)?);
        let (da, db) = (px.dist(pa), px.dist(pb));
        let e = (fb - fa).normalized()?;
        let d = fa.dist(fb);
        let along = (da * da - db * db + d * d) / (2.0 * d);
        let off = (da * da - along * along).max(0.0).sqrt();
        let side = (pb - pa).cross(px - pa).dot(*normals.get(x)?).signum();
        *flat.get_mut(x)? = Some(fa + e * along + e.perp() * (off * side));
        Some(())
    };
    // The first two rows as a strip, then each row from the one before.
    place(at(0, 1), at(0, 0), at(1, 0))?;
    for i in 1..nu {
        if i > 1 {
            place(at(i, 0), at(i - 1, 0), at(i - 1, 1))?;
        }
        place(at(i, 1), at(i - 1, 1), at(i, 0))?;
    }
    for j in 2..nv {
        place(at(0, j), at(0, j - 1), at(1, j - 1))?;
        for i in 1..nu {
            place(at(i, j), at(i - 1, j), at(i, j - 1))?;
        }
    }
    let flat: Vec<Vec2> = flat.into_iter().collect::<Option<_>>()?;
    // Least squares in the space, slightly damped toward nothing so that it is always solvable.
    let count = spaces[0].count() * spaces[1].count();
    let mut ata = vec![0.0; count * count];
    let mut atb = vec![Vec2::ZERO; count];
    for (prm, x) in params.iter().zip(&flat) {
        let row = basis_row(spaces, *prm, (0, 0))?;
        for &(a, wa) in &row {
            *atb.get_mut(a)? += *x * wa;
            for &(b, wb) in &row {
                *ata.get_mut(a * count + b)? += wa * wb;
            }
        }
    }
    for k in 0..count {
        *ata.get_mut(k * count + k)? += 1e-12;
    }
    solve_dense(&mut ata, &mut atb, count)?;
    Some(atb)
}

/// The tensor basis functions that do not vanish at `prm`, with the `der` derivative orders (in
/// u, in v): (control point index, value).
fn basis_row(spaces: &[Space; 2], prm: Vec2, der: (usize, usize)) -> Option<Vec<(usize, f64)>> {
    let nu = spaces[0].count();
    let (iu, bu) = spaces[0].basis(prm.x, der.0).ok()?;
    let (iv, bv) = spaces[1].basis(prm.y, der.1).ok()?;
    Some(bv.iter().enumerate().flat_map(|(j, wj)| bu.iter().enumerate().map(move |(i, wi)| ((iv + j) * nu + iu + i, wi * wj))).collect())
}

/// Damped Gauss-Newton on the metric: F's first fundamental form against the neutral surface's
/// at every sample, each residual scaled to a strain.
fn gauss_newton(spaces: &[Space; 2], mut ctrl: Vec<Vec2>, pts: &[(Vec2, [f64; 3])]) -> Vec<Vec2> {
    let count = ctrl.len();
    let unknowns = 2 * count;
    if unknowns > MAX_UNKNOWNS {
        return ctrl;
    }
    let mut lambda = 1e-6;
    let mut cost = metric_cost(spaces, &ctrl, pts);
    for _ in 0..MAX_STEPS {
        let mut jtj = vec![0.0; unknowns * unknowns];
        let mut jtr = vec![0.0; unknowns];
        for (prm, [e, f, g]) in pts {
            let (Some(ru), Some(rv)) = (basis_row(spaces, *prm, (1, 0)), basis_row(spaces, *prm, (0, 1))) else { return ctrl };
            let fu = ru.iter().fold(Vec2::ZERO, |s, &(k, w)| s + ctrl.get(k).copied().unwrap_or_default() * w);
            let fv = rv.iter().fold(Vec2::ZERO, |s, &(k, w)| s + ctrl.get(k).copied().unwrap_or_default() * w);
            let (se, sf, sg) = (1.0 / e.max(1e-300), 1.0 / (e * g).sqrt().max(1e-300), 1.0 / g.max(1e-300));
            let residuals = [((fu.dot(fu) - e) * se, 0), ((fu.dot(fv) - f) * sf, 1), ((fv.dot(fv) - g) * sg, 2)];
            for (r, which) in residuals {
                // The residual's gradient: one 2-vector per control point it touches.
                let mut grad: Vec<(usize, Vec2)> = Vec::new();
                match which {
                    0 => grad.extend(ru.iter().map(|&(k, w)| (k, fu * (2.0 * w * se)))),
                    1 => {
                        grad.extend(ru.iter().map(|&(k, w)| (k, fv * (w * sf))));
                        grad.extend(rv.iter().map(|&(k, w)| (k, fu * (w * sf))));
                    }
                    _ => grad.extend(rv.iter().map(|&(k, w)| (k, fv * (2.0 * w * sg)))),
                }
                for &(a, ga) in &grad {
                    for (ca, va) in [(2 * a, ga.x), (2 * a + 1, ga.y)] {
                        if let Some(x) = jtr.get_mut(ca) {
                            *x += va * r;
                        }
                        for &(b, gb) in &grad {
                            for (cb, vb) in [(2 * b, gb.x), (2 * b + 1, gb.y)] {
                                if let Some(x) = jtj.get_mut(ca * unknowns + cb) {
                                    *x += va * vb;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Levenberg-Marquardt: damp, try, and keep the step only if the metric fits better. The
        // damping also pins the free rigid motion of the flat.
        let mut improved = false;
        for _ in 0..8 {
            let mut m = jtj.clone();
            for k in 0..unknowns {
                if let Some(x) = m.get_mut(k * unknowns + k) {
                    *x += lambda * (1.0 + *x);
                }
            }
            let mut step: Vec<f64> = jtr.iter().map(|x| -x).collect();
            if solve_dense(&mut m, &mut step, unknowns).is_none() {
                lambda *= 10.0;
                continue;
            }
            let trial: Vec<Vec2> = ctrl
                .iter()
                .enumerate()
                .map(|(k, c)| *c + Vec2::new(step.get(2 * k).copied().unwrap_or(0.0), step.get(2 * k + 1).copied().unwrap_or(0.0)))
                .collect();
            let tc = metric_cost(spaces, &trial, pts);
            if tc < cost {
                improved = cost - tc > 1e-30;
                ctrl = trial;
                cost = tc;
                lambda = (lambda * 0.3).max(1e-12);
                break;
            }
            lambda *= 10.0;
        }
        if !improved || cost < 1e-28 {
            break;
        }
    }
    ctrl
}

/// The sum of the squared strain residuals of F's metric against the samples'.
fn metric_cost(spaces: &[Space; 2], ctrl: &[Vec2], pts: &[(Vec2, [f64; 3])]) -> f64 {
    let map = FlatMap { spaces: spaces.clone(), ctrl: ctrl.to_vec(), stretch: 0.0 };
    pts.iter()
        .map(|(prm, [e, f, g])| match map.eval(prm.x, prm.y) {
            Some([_, fu, fv]) => ((fu.dot(fu) - e) / e).powi(2) + ((fu.dot(fv) - f) / (e * g).sqrt()).powi(2) + ((fv.dot(fv) - g) / g).powi(2),
            None => f64::INFINITY,
        })
        .sum()
}

/// The largest principal strain of F against the neutral surface over the samples: the square
/// roots of the eigenvalues of (neutral metric)^-1 (F's metric), less one.
fn strain(spaces: &[Space; 2], ctrl: &[Vec2], pts: &[(Vec2, [f64; 3])]) -> f64 {
    let map = FlatMap { spaces: spaces.clone(), ctrl: ctrl.to_vec(), stretch: 0.0 };
    pts.iter()
        .map(|(prm, [e, f, g])| {
            let Some([_, fu, fv]) = map.eval(prm.x, prm.y) else { return f64::INFINITY };
            let (a, b, c) = (fu.dot(fu), fu.dot(fv), fv.dot(fv));
            let det = e * g - f * f;
            // (I_n)^-1 I_F, a 2 x 2 matrix: its trace and determinant give its eigenvalues.
            let (m00, m01, m10, m11) = ((g * a - f * b) / det, (g * b - f * c) / det, (e * b - f * a) / det, (e * c - f * b) / det);
            let (tr, dt) = (m00 + m11, m00 * m11 - m01 * m10);
            let disc = (tr * tr / 4.0 - dt).max(0.0).sqrt();
            [tr / 2.0 + disc, tr / 2.0 - disc].iter().map(|l| (l.max(0.0).sqrt() - 1.0).abs()).fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}
