//! The NURBS surface: validation, evaluation with first derivatives, and projection back onto it
//! (The NURBS Book, A2.1 to A2.3).

use crate::MeshError;

/// A NURBS surface. Poles are indexed `i * nv + j`, `i` along u and `j` along v.
#[derive(Debug, Clone)]
pub struct NurbsSurface {
    pub(crate) u_degree: usize,
    pub(crate) v_degree: usize,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
    pub(crate) nu: usize,
    pub(crate) nv: usize,
    /// Cartesian poles, for the curvature bound and for seeding.
    pub(crate) poles: Vec<[f64; 3]>,
    /// The poles in homogeneous form, `[x w, y w, z w, w]`, for evaluation.
    pub(crate) hpoles: Vec<[f64; 4]>,
    pub(crate) rational: bool,
    domain: [[f64; 2]; 2],
}

/// A point on the surface with its first partial derivatives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePoint {
    /// S(u, v).
    pub point: [f64; 3],
    /// dS/du.
    pub du: [f64; 3],
    /// dS/dv.
    pub dv: [f64; 3],
}

impl NurbsSurface {
    /// A surface from flat knot vectors (lengths `nu + u_degree + 1` and `nv + v_degree + 1`),
    /// `nu * nv` poles and, for a rational surface, one positive weight per pole. Every size and
    /// value is checked here, so evaluation never meets a short array.
    pub fn new(
        u_degree: usize,
        v_degree: usize,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        nu: usize,
        nv: usize,
        poles: Vec<[f64; 3]>,
        weights: Option<&[f64]>,
    ) -> Result<Self, MeshError> {
        if u_degree == 0 || v_degree == 0 {
            return Err(MeshError::BadSurface("degree 0"));
        }
        if nu <= u_degree || nv <= v_degree {
            return Err(MeshError::BadSurface("fewer poles than degree + 1"));
        }
        if u_knots.len() != nu + u_degree + 1 || v_knots.len() != nv + v_degree + 1 {
            return Err(MeshError::BadSurface(
                "knot count is not poles + degree + 1",
            ));
        }
        if poles.len() != nu * nv {
            return Err(MeshError::BadSurface("pole count is not nu * nv"));
        }
        for k in [&u_knots, &v_knots] {
            if k.iter().any(|x| !x.is_finite())
                || k.windows(2).any(|w| matches!(w, [a, b] if b < a))
            {
                return Err(MeshError::BadSurface("knots not finite and nondecreasing"));
            }
        }
        if poles.iter().flatten().any(|x| !x.is_finite()) {
            return Err(MeshError::BadSurface("pole not finite"));
        }
        let hpoles: Vec<[f64; 4]> = match weights {
            None => poles.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
            Some(w) => {
                if w.len() != poles.len() {
                    return Err(MeshError::BadSurface("weight count is not nu * nv"));
                }
                if w.iter().any(|&x| !(x.is_finite() && x > 0.0)) {
                    return Err(MeshError::BadSurface("weight not finite and positive"));
                }
                poles
                    .iter()
                    .zip(w)
                    .map(|(&[x, y, z], &w)| [x * w, y * w, z * w, w])
                    .collect()
            }
        };
        let end = |k: &[f64], i: usize| k.get(i).copied().ok_or(MeshError::Internal);
        let domain = [
            [end(&u_knots, u_degree)?, end(&u_knots, nu)?],
            [end(&v_knots, v_degree)?, end(&v_knots, nv)?],
        ];
        let [[du0, du1], [dv0, dv1]] = domain;
        if !(du0 < du1 && dv0 < dv1) {
            return Err(MeshError::BadSurface("empty parameter domain"));
        }
        Ok(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            nu,
            nv,
            poles,
            hpoles,
            rational: weights.is_some(),
            domain,
        })
    }

    /// The parameter domain, `[[u0, u1], [v0, v1]]`.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        self.domain
    }

    /// Degrees in u and v.
    pub fn degrees(&self) -> [usize; 2] {
        [self.u_degree, self.v_degree]
    }

    /// Pole counts in u and v.
    pub fn pole_counts(&self) -> [usize; 2] {
        [self.nu, self.nv]
    }

    /// True when the surface carries weights.
    pub fn is_rational(&self) -> bool {
        self.rational
    }

    /// The surface and its first partial derivatives at (u, v). Parameters outside the domain
    /// evaluate the end spans' polynomials.
    pub fn evaluate(&self, u: f64, v: f64) -> Result<SurfacePoint, MeshError> {
        let (p, q) = (self.u_degree, self.v_degree);
        let iu = find_span(self.nu, p, u, &self.u_knots).ok_or(MeshError::Internal)?;
        let iv = find_span(self.nv, q, v, &self.v_knots).ok_or(MeshError::Internal)?;
        let (bu, du) = basis(iu, u, p, &self.u_knots).ok_or(MeshError::Internal)?;
        let (bv, dv) = basis(iv, v, q, &self.v_knots).ok_or(MeshError::Internal)?;
        // Homogeneous sums A = sum N h, with their u and v derivatives.
        let mut a = [0.0; 4];
        let mut au = [0.0; 4];
        let mut av = [0.0; 4];
        for (r, (&nu, &nu1)) in bu.iter().zip(&du).enumerate() {
            let row = (iu + r - p) * self.nv + iv - q;
            let poles = self.hpoles.get(row..=row + q).ok_or(MeshError::Internal)?;
            for ((h, &nv), &nv1) in poles.iter().zip(&bv).zip(&dv) {
                axpy(&mut a, nu * nv, h);
                axpy(&mut au, nu1 * nv, h);
                axpy(&mut av, nu * nv1, h);
            }
        }
        let [ax, ay, az, w] = a;
        let point = [ax / w, ay / w, az / w];
        let deriv = |[x, y, z, dw]: [f64; 4]| {
            let [px, py, pz] = point;
            [(x - dw * px) / w, (y - dw * py) / w, (z - dw * pz) / w]
        };
        Ok(SurfacePoint {
            point,
            du: deriv(au),
            dv: deriv(av),
        })
    }

    /// The (u, v) that Gauss-Newton reaches from `seed` toward the foot of `q`, clamped to the
    /// domain, and the distance from `q` to the surface point there. A local search: the distance
    /// is to a real surface point, so it bounds the distance from `q` to the surface from above.
    pub fn foot_near(&self, q: [f64; 3], seed: [f64; 2]) -> Result<([f64; 2], f64), MeshError> {
        let [[u0, u1], [v0, v1]] = self.domain;
        let [mut u, mut v] = seed;
        for _ in 0..30 {
            let SurfacePoint {
                point,
                du: su,
                dv: sv,
            } = self.evaluate(u, v)?;
            let r = sub(q, point);
            let (a, b, c) = (dot(su, su), dot(su, sv), dot(sv, sv));
            let (e, f) = (dot(su, r), dot(sv, r));
            let det = a * c - b * b;
            if det == 0.0 || !det.is_finite() {
                break;
            }
            let (mut nu, mut nv) = (u + (c * e - b * f) / det, v + (a * f - b * e) / det);
            // A joint step that leaves the domain in one parameter: hold that one at the edge and
            // solve for the other alone. Clamping the joint step instead keeps a v (or u) that is
            // right only if the clamped parameter had moved, and settles on the wrong point.
            let (out_u, out_v) = (!(u0..=u1).contains(&nu), !(v0..=v1).contains(&nv));
            if out_u && !out_v && c > 0.0 {
                nu = nu.clamp(u0, u1);
                nv = v + f / c;
            } else if out_v && !out_u && a > 0.0 {
                nv = nv.clamp(v0, v1);
                nu = u + e / a;
            }
            let (nu, nv) = (nu.clamp(u0, u1), nv.clamp(v0, v1));
            let (step_u, step_v) = (nu - u, nv - v);
            u = nu;
            v = nv;
            if step_u.abs() < 1e-12 * (u1 - u0) && step_v.abs() < 1e-12 * (v1 - v0) {
                break;
            }
        }
        let point = self.evaluate(u, v)?.point;
        Ok(([u, v], norm(sub(q, point))))
    }

    /// The (u, v) of a point on the surface (within `tol`), searched from `seed`. `None` when the
    /// search does not get within `tol`: a point that is not on the surface has no business on
    /// one of its faces' boundaries.
    pub fn project(
        &self,
        q: [f64; 3],
        seed: [f64; 2],
        tol: f64,
    ) -> Result<Option<[f64; 2]>, MeshError> {
        let (uv, d) = self.foot_near(q, seed)?;
        Ok((d <= tol).then_some(uv))
    }

    /// A seed for [`project`](Self::project): the Greville abscissae of the pole nearest `q`.
    pub fn seed_near(&self, q: [f64; 3]) -> Result<[f64; 2], MeshError> {
        let best = self
            .poles
            .iter()
            .map(|&p| norm2(sub(p, q)))
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(k, _)| k);
        let greville = |k: &[f64], p: usize, i: usize| {
            k.get(i + 1..=i + p)
                .map(|s| s.iter().sum::<f64>() / p as f64)
                .ok_or(MeshError::Internal)
        };
        Ok([
            greville(&self.u_knots, self.u_degree, best / self.nv)?,
            greville(&self.v_knots, self.v_degree, best % self.nv)?,
        ])
    }
}

/// The knot span holding `t`: the `i` with `k[i] <= t < k[i + 1]`, clamped to `p..n`.
pub(crate) fn find_span(n: usize, p: usize, t: f64, k: &[f64]) -> Option<usize> {
    if !t.is_finite() {
        return None;
    }
    if t >= *k.get(n)? {
        return Some(n - 1);
    }
    if t <= *k.get(p)? {
        return Some(p);
    }
    let (mut lo, mut hi) = (p, n);
    // Bisection; 64 halvings cover any usize range.
    for _ in 0..64 {
        let mid = (lo + hi) / 2;
        if t < *k.get(mid)? {
            hi = mid;
        } else if t >= *k.get(mid + 1)? {
            lo = mid;
        } else {
            return Some(mid);
        }
    }
    None
}

/// The `p + 1` basis functions that are nonzero on span `i` at `t`, and their first derivatives.
pub(crate) fn basis(i: usize, t: f64, p: usize, k: &[f64]) -> Option<(Vec<f64>, Vec<f64>)> {
    let w = p + 1;
    // ndu[r][c] at r * w + c: basis values above the diagonal, knot differences below it.
    let mut ndu = vec![0.0; w * w];
    let mut left = vec![0.0; w];
    let mut right = vec![0.0; w];
    *ndu.get_mut(0)? = 1.0;
    for j in 1..=p {
        *left.get_mut(j)? = t - k.get((i + 1).checked_sub(j)?)?;
        *right.get_mut(j)? = k.get(i + j)? - t;
        let mut saved = 0.0;
        for r in 0..j {
            let lower = right.get(r + 1)? + left.get(j - r)?;
            *ndu.get_mut(j * w + r)? = lower;
            let tmp = ndu.get(r * w + j - 1)? / lower;
            *ndu.get_mut(r * w + j)? = saved + right.get(r + 1)? * tmp;
            saved = left.get(j - r)? * tmp;
        }
        *ndu.get_mut(j * w + j)? = saved;
    }
    let n = (0..w)
        .map(|r| ndu.get(r * w + p).copied())
        .collect::<Option<Vec<_>>>()?;
    let mut d = vec![0.0; w];
    for (r, dr) in d.iter_mut().enumerate() {
        let mut x = 0.0;
        if r >= 1 {
            x += ndu.get((r - 1) * w + p - 1)? / ndu.get(p * w + r - 1)?;
        }
        if r < p {
            x -= ndu.get(r * w + p - 1)? / ndu.get(p * w + r)?;
        }
        *dr = x * p as f64;
    }
    Some((n, d))
}

fn axpy(acc: &mut [f64; 4], s: f64, h: &[f64; 4]) {
    for (a, x) in acc.iter_mut().zip(h) {
        *a += s * x;
    }
}

pub(crate) fn sub([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> [f64; 3] {
    [ax - bx, ay - by, az - bz]
}

pub(crate) fn dot([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> f64 {
    ax * bx + ay * by + az * bz
}

pub(crate) fn norm2(a: [f64; 3]) -> f64 {
    dot(a, a)
}

pub(crate) fn norm(a: [f64; 3]) -> f64 {
    norm2(a).sqrt()
}

pub(crate) fn lerp([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3], s: f64) -> [f64; 3] {
    [ax + s * (bx - ax), ay + s * (by - ay), az + s * (bz - az)]
}
