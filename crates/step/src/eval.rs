//! Points on STEP curves and surfaces, in STEP's own parameterisation (ISO 10303-42), so tests
//! and the topology reader can check that vertices lie on their curves. `None` for a parameter
//! the geometry is not defined at, or for geometry too inconsistent to evaluate.

use crate::geom::{BSplineCurve, BSplineSurface, Curve, P3, Surface, add, cross, dot, norm, scale, sub};

/// Every knot repeated by its multiplicity.
fn expanded(knots: &[f64], mults: &[usize]) -> Option<Vec<f64>> {
    if knots.len() != mults.len() {
        return None;
    }
    let total = mults.iter().try_fold(0usize, |a, m| a.checked_add(*m))?;
    let mut v = Vec::with_capacity(total);
    for (k, m) in knots.iter().zip(mults) {
        v.extend(std::iter::repeat_n(*k, *m));
    }
    Some(v)
}

/// Index `i` of the knot span `[U[i], U[i+1])` holding `t`, with `degree <= i < n` for `n`
/// control points (the last span takes the domain's upper end).
fn span(u: &[f64], degree: usize, n: usize, t: f64) -> Option<usize> {
    let lo = *u.get(degree)?;
    let hi = *u.get(n)?;
    if !(t >= lo - 1e-12 * (1.0 + lo.abs()) && t <= hi + 1e-12 * (1.0 + hi.abs())) {
        return None;
    }
    let mut i = degree;
    while i + 1 < n && *u.get(i + 1)? <= t {
        i += 1;
    }
    Some(i)
}

/// De Boor on homogeneous points `[x·w, y·w, z·w, w]`.
fn de_boor(u: &[f64], degree: usize, ctrl: &[[f64; 4]], t: f64) -> Option<[f64; 4]> {
    let k = span(u, degree, ctrl.len(), t)?;
    let mut d: Vec<[f64; 4]> = (0..=degree).map(|j| ctrl.get(j + k - degree).copied()).collect::<Option<_>>()?;
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let a = *u.get(i)?;
            let b = *u.get(i + 1 + degree - r)?;
            let alpha = if b - a > 0.0 { (t - a) / (b - a) } else { 0.0 };
            let p = *d.get(j - 1)?;
            let q = *d.get(j)?;
            *d.get_mut(j)? = [0, 1, 2, 3].map(|c| (1.0 - alpha) * p[c] + alpha * q[c]);
        }
    }
    d.get(degree).copied()
}

fn from_homogeneous(h: [f64; 4]) -> Option<P3> {
    (h[3].abs() > 1e-300).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).filter(|p| p.iter().all(|x| x.is_finite()))
}

fn homogeneous(p: P3, w: f64) -> [f64; 4] {
    [p[0] * w, p[1] * w, p[2] * w, w]
}

impl BSplineCurve {
    /// Parameter range: from knot `degree` to knot `n` of the expanded knot vector.
    pub fn domain(&self) -> Option<(f64, f64)> {
        let u = expanded(&self.knots, &self.multiplicities)?;
        Some((*u.get(self.degree)?, *u.get(self.points.len())?))
    }
    pub fn point(&self, t: f64) -> Option<P3> {
        let u = expanded(&self.knots, &self.multiplicities)?;
        if u.len() != self.points.len() + self.degree + 1 {
            return None;
        }
        let ctrl: Vec<[f64; 4]> = match &self.weights {
            Some(w) if w.len() == self.points.len() => self.points.iter().zip(w).map(|(p, w)| homogeneous(*p, *w)).collect(),
            Some(_) => return None,
            None => self.points.iter().map(|p| homogeneous(*p, 1.0)).collect(),
        };
        from_homogeneous(de_boor(&u, self.degree, &ctrl, t)?)
    }
}

impl BSplineSurface {
    pub fn point(&self, s: f64, t: f64) -> Option<P3> {
        let uk = expanded(&self.u_knots, &self.u_multiplicities)?;
        let vk = expanded(&self.v_knots, &self.v_multiplicities)?;
        let nu = self.points.len();
        let nv = self.points.first()?.len();
        if uk.len() != nu + self.u_degree + 1 || vk.len() != nv + self.v_degree + 1 {
            return None;
        }
        // Each u row evaluated along v, then the column of results along u.
        let mut column = Vec::with_capacity(nu);
        for (i, row) in self.points.iter().enumerate() {
            if row.len() != nv {
                return None;
            }
            let ctrl: Vec<[f64; 4]> = match &self.weights {
                Some(w) => {
                    let w = w.get(i).filter(|w| w.len() == nv)?;
                    row.iter().zip(w).map(|(p, w)| homogeneous(*p, *w)).collect()
                }
                None => row.iter().map(|p| homogeneous(*p, 1.0)).collect(),
            };
            column.push(de_boor(&vk, self.v_degree, &ctrl, t)?);
        }
        from_homogeneous(de_boor(&uk, self.u_degree, &column, s)?)
    }
}

impl Curve {
    /// The point at parameter `t`.
    pub fn point(&self, t: f64) -> Option<P3> {
        self.point_at(t, 0)
    }

    fn point_at(&self, t: f64, depth: usize) -> Option<P3> {
        if depth > 16 || !t.is_finite() {
            return None;
        }
        let p = match self {
            Curve::Line { origin, direction, magnitude } => add(*origin, scale(*direction, t * magnitude)),
            Curve::Circle { position, radius } => position.at(radius * t.cos(), radius * t.sin(), 0.0),
            Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => position.at(semi_axis_1 * t.cos(), semi_axis_2 * t.sin(), 0.0),
            Curve::Hyperbola { position, semi_axis, semi_imag_axis } => position.at(semi_axis * t.cosh(), semi_imag_axis * t.sinh(), 0.0),
            Curve::Parabola { position, focal_dist } => position.at(focal_dist * t * t, 2.0 * focal_dist * t, 0.0),
            Curve::BSpline(b) => b.point(t)?,
            Curve::Polyline(pts) => {
                if pts.len() < 2 || t < 0.0 || t > (pts.len() - 1) as f64 {
                    return None;
                }
                let i = (t.floor() as usize).min(pts.len() - 2);
                let (a, b) = (*pts.get(i)?, *pts.get(i + 1)?);
                add(a, scale(sub(b, a), t - i as f64))
            }
            Curve::Trimmed { basis, .. } => basis.point_at(t, depth + 1)?,
            Curve::SurfaceCurve { curve_3d, .. } => curve_3d.point_at(t, depth + 1)?,
            // Needs the basis curve's derivative; not evaluated yet.
            Curve::OffsetCurve3d { .. } => return None,
        };
        p.iter().all(|x| x.is_finite()).then_some(p)
    }
}

impl Surface {
    /// The point at parameters `(u, v)`.
    pub fn point(&self, u: f64, v: f64) -> Option<P3> {
        self.point_at(u, v, 0)
    }

    /// The unit normal `∂S/∂u × ∂S/∂v` at `(u, v)`, by central differences.
    pub fn normal(&self, u: f64, v: f64) -> Option<P3> {
        self.normal_at(u, v, 0)
    }

    fn normal_at(&self, u: f64, v: f64, depth: usize) -> Option<P3> {
        let h = 1e-6;
        let du = sub(self.point_at(u + h, v, depth)?, self.point_at(u - h, v, depth)?);
        let dv = sub(self.point_at(u, v + h, depth)?, self.point_at(u, v - h, depth)?);
        let n = cross(du, dv);
        let m = norm(n);
        (m > 1e-30 && m.is_finite()).then(|| scale(n, 1.0 / m))
    }

    fn point_at(&self, u: f64, v: f64, depth: usize) -> Option<P3> {
        if depth > 16 || !u.is_finite() || !v.is_finite() {
            return None;
        }
        let p = match self {
            Surface::Plane { position } => position.at(u, v, 0.0),
            Surface::Cylinder { position, radius } => position.at(radius * u.cos(), radius * u.sin(), v),
            Surface::Cone { position, radius, semi_angle } => {
                let r = radius + v * semi_angle.tan();
                position.at(r * u.cos(), r * u.sin(), v)
            }
            Surface::Sphere { position, radius } => position.at(radius * v.cos() * u.cos(), radius * v.cos() * u.sin(), radius * v.sin()),
            Surface::Torus { position, major_radius, minor_radius } | Surface::DegenerateTorus { position, major_radius, minor_radius, .. } => {
                let r = major_radius + minor_radius * v.cos();
                position.at(r * u.cos(), r * u.sin(), minor_radius * v.sin())
            }
            Surface::BSpline(b) => b.point(u, v)?,
            Surface::Revolution { profile, axis } => {
                // Rodrigues: the profile point turned by u about the axis.
                let q = sub(profile.point_at(v, depth + 1)?, axis.origin);
                let k = axis.direction;
                let (s, c) = u.sin_cos();
                add(axis.origin, add(add(scale(q, c), scale(cross(k, q), s)), scale(k, dot(k, q) * (1.0 - c))))
            }
            Surface::Extrusion { profile, direction, magnitude } => add(profile.point_at(u, depth + 1)?, scale(*direction, v * magnitude)),
            Surface::Offset { basis, distance, .. } => add(basis.point_at(u, v, depth + 1)?, scale(basis.normal_at(u, v, depth + 1)?, *distance)),
            Surface::RectangularTrimmed { basis, .. } => basis.point_at(u, v, depth + 1)?,
        };
        p.iter().all(|x| x.is_finite()).then_some(p)
    }
}
