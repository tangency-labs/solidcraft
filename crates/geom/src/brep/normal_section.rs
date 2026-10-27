//! The curvature of a surface's normal section and its first two derivatives by arc length, from
//! the surface's exact partial derivatives ([`Partials`]): what a seam's G2, G3 and G4 compare.
//!
//! At a point `p` with unit normal `n`, the normal section along a unit tangent direction `w` is
//! the curve the plane through `p` spanned by `n` and `w` cuts from the surface. Written as a
//! height over that plane's `w` axis, `h(x) = (r − p) · n` with `x = (r − p) · w`, it starts flat
//! (`h(0) = h′(0) = 0`), and its signed curvature `κ = h″ / (1 + h′²)^(3/2)` has, at `p`,
//!
//! - `κ = h″` (the normal curvature in direction `w`: `n · r″`),
//! - `dκ/ds = h‴` (a curvature that changes along `w`: the G3 quantity),
//! - `d²κ/ds² = h⁗ − 3 h″³` (the G4 quantity).
//!
//! The height's Taylor coefficients come from the surface's: the section is the curve
//! `(u(x), v(x))` on which `(r − p) · w = x` and `(r − p) · t = 0` (`t = w × n`, the cutting
//! plane's normal). Expanding `r` about `p` in its partials and solving those two conditions order
//! by order gives each coefficient of `u` and `v` from the 2 × 2 system of the first partials
//! (the lower orders' terms on the right), and the height's coefficient is the normal part of the
//! same expansion. Nothing is sampled: the answer is exact up to rounding.

use crate::Vec3;
use crate::tess::partials::{MAX_PARTIAL_ORDER, Partials, orders};

/// Taylor coefficients in `x` up to the fourth power (index = power).
type Series<T> = [T; MAX_PARTIAL_ORDER + 1];

/// The normal section's curvature and its first two arc-length derivatives at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionJet {
    /// `κ = n · r″`, 1/mm: negative where the surface bends away from `n`.
    pub curvature: f64,
    /// `dκ/ds` along the direction, 1/mm².
    pub curvature_change: f64,
    /// `d²κ/ds²` along the direction, 1/mm³.
    pub curvature_change_rate: f64,
}

impl SectionJet {
    /// The three values in order: curvature, its change, the change's rate.
    pub fn values(&self) -> [f64; 3] {
        [self.curvature, self.curvature_change, self.curvature_change_rate]
    }
}

/// The normal section at the point of `d` (`d[0][0]`) with unit normal `normal`, along the unit
/// tangent `direction`. `None` where the first partials do not span the tangent plane across the
/// section (a pole, a degenerate point) or a value is not finite.
pub fn normal_section(d: &Partials, normal: Vec3, direction: Vec3) -> Option<SectionJet> {
    let cut = direction.cross(normal);
    let (ru, rv) = (d[1][0], d[0][1]);
    // [ru·w  rv·w; ru·t  rv·t] [a; b] = rhs, solved by Cramer's rule.
    let (m11, m12, m21, m22) = (ru.dot(direction), rv.dot(direction), ru.dot(cut), rv.dot(cut));
    let det = m11 * m22 - m12 * m21;
    let scale = ru.len() * rv.len();
    if !(det.abs() > 1e-12 * scale && scale.is_finite()) {
        return None;
    }
    let mut du: Series<f64> = [0.0; MAX_PARTIAL_ORDER + 1];
    let mut dv: Series<f64> = [0.0; MAX_PARTIAL_ORDER + 1];
    let mut height: Series<f64> = [0.0; MAX_PARTIAL_ORDER + 1];
    for k in 1..=MAX_PARTIAL_ORDER {
        // The terms of order two and up, which only reach x^k through the coefficients below k.
        let rest = *nonlinear(d, &du, &dv).get(k)?;
        let target = if k == 1 { 1.0 } else { 0.0 };
        let (r1, r2) = (target - rest.dot(direction), -rest.dot(cut));
        *du.get_mut(k)? = (r1 * m22 - m12 * r2) / det;
        *dv.get_mut(k)? = (m11 * r2 - r1 * m21) / det;
        *height.get_mut(k)? = rest.dot(normal);
    }
    let [_, _, h2, h3, h4] = height;
    let (curvature, change) = (2.0 * h2, 6.0 * h3);
    let rate = 24.0 * h4 - 3.0 * curvature.powi(3);
    let jet = SectionJet { curvature, curvature_change: change, curvature_change_rate: rate };
    jet.values().iter().all(|x| x.is_finite()).then_some(jet)
}

/// `Σ_{2 ≤ i+j ≤ 4} d[i][j] du^i dv^j / (i! j!)`, as a series in `x`.
fn nonlinear(d: &Partials, du: &Series<f64>, dv: &Series<f64>) -> Series<Vec3> {
    let pu = powers(du);
    let pv = powers(dv);
    let mut out: Series<Vec3> = [Vec3::ZERO; MAX_PARTIAL_ORDER + 1];
    for (i, j) in orders().filter(|(i, j)| i + j >= 2) {
        let term = times(&pu[i], &pv[j]);
        let weight = 1.0 / (factorial(i) * factorial(j));
        for (o, t) in out.iter_mut().zip(term) {
            *o = *o + d[i][j] * (t * weight);
        }
    }
    out
}

/// `s^0 … s^4`, truncated to the fourth power of `x`.
fn powers(s: &Series<f64>) -> [Series<f64>; MAX_PARTIAL_ORDER + 1] {
    let mut out = [[0.0; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    out[0][0] = 1.0;
    for k in 1..=MAX_PARTIAL_ORDER {
        out[k] = times(&out[k - 1], s);
    }
    out
}

/// The product of two series, truncated to the fourth power.
fn times(a: &Series<f64>, b: &Series<f64>) -> Series<f64> {
    let mut out = [0.0; MAX_PARTIAL_ORDER + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate().take(MAX_PARTIAL_ORDER + 1 - i) {
            out[i + j] += x * y;
        }
    }
    out
}

fn factorial(n: usize) -> f64 {
    (1..=n).map(|k| k as f64).product()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::brep::eval::SurfaceEval;
    use crate::brep::{Frame, Surface};

    fn frame() -> Frame {
        Frame { origin: [1., 2., 3.], x_axis: [0., 1., 0.], y_axis: [0., 0., 1.] }
    }

    fn jet(s: &Surface, uv: [f64; 2], along: impl Fn(Vec3, Vec3) -> Vec3) -> SectionJet {
        let e = SurfaceEval::new(s).unwrap();
        let d = e.partials(uv[0], uv[1]).unwrap();
        let n = d[1][0].cross(d[0][1]).normalized().unwrap();
        let w = along(d[1][0].normalized().unwrap(), d[0][1].normalized().unwrap());
        normal_section(&d, n, w).unwrap()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * (1.0 + b.abs())
    }

    /// A sphere's normal sections are great circles: curvature 1/R (towards the centre, against
    /// the outward normal), constant, in every direction.
    #[test]
    fn a_sphere_is_a_constant_curvature_every_way() {
        let s = Surface::Sphere { frame: frame(), radius: 4.0 };
        for along in [|u: Vec3, _: Vec3| u, |_: Vec3, v: Vec3| v, |u: Vec3, v: Vec3| (u + v).normalized().unwrap_or(u)] {
            let j = jet(&s, [0.7, 0.3], along);
            assert!(near(j.curvature, -0.25) && near(j.curvature_change, 0.0) && near(j.curvature_change_rate, 0.0), "{j:?}");
        }
    }

    /// A cylinder across its axis: a circle (-1/R, constant); along it: a straight line.
    #[test]
    fn a_cylinder_is_a_circle_across_and_a_line_along() {
        let s = Surface::Cylinder { frame: frame(), radius: 5.0 };
        let across = jet(&s, [1.1, 2.0], |u, _| u);
        assert!(near(across.curvature, -0.2) && near(across.curvature_change, 0.0) && near(across.curvature_change_rate, 0.0), "{across:?}");
        let along = jet(&s, [1.1, 2.0], |_, v| v);
        assert!(along.values().iter().all(|x| x.abs() < 1e-12), "{along:?}");
    }

    /// The graph `z = x³/6 + x⁴/12`, an exact quartic Bézier in x and straight in y, has h″ = 0,
    /// h‴ = 1 and h⁗ = 2 at the origin: curvature 0, its change 1 and the change's rate 2 along x.
    #[test]
    fn a_polynomial_graph_reads_its_own_derivatives() {
        let z = |x: f64| x.powi(3) / 6.0 + x.powi(4) / 12.0;
        // Monomial coefficients a0..a4 = [0, 0, 0, 1/6, 1/12]; Bézier b_i = Σ_k C(i,k)/C(4,k) a_k.
        let a = [0.0, 0.0, 0.0, 1.0 / 6.0, 1.0 / 12.0];
        let c = |n: usize, k: usize| crate::bspline::binomial(n, k);
        let b: Vec<f64> = (0..=4).map(|i| (0..=i).map(|k| c(i, k) / c(4, k) * a[k]).sum()).collect();
        let mut points = Vec::new();
        for (i, bi) in b.iter().enumerate() {
            points.push(vec![[i as f64 / 4.0, 0.0, *bi], [i as f64 / 4.0, 1.0, *bi]]);
        }
        let s = Surface::Spline { u_knots: vec![0., 0., 0., 0., 0., 1., 1., 1., 1., 1.], v_knots: vec![0., 0., 1., 1.], points };
        let e = SurfaceEval::new(&s).unwrap();
        assert!(near(e.point([0.5, 0.5]).unwrap().z, z(0.5)));
        let d = e.partials(0.0, 0.5).unwrap();
        let j = normal_section(&d, Vec3::Z, Vec3::X).unwrap();
        assert!(near(j.curvature, 0.0) && near(j.curvature_change, 1.0) && near(j.curvature_change_rate, 2.0), "{j:?}");
    }

    #[test]
    fn a_degenerate_point_has_no_section() {
        let mut d: Partials = [[Vec3::ZERO; 5]; 5];
        d[1][0] = Vec3::X;
        assert!(normal_section(&d, Vec3::Z, Vec3::X).is_none(), "r_v is zero");
    }
}
