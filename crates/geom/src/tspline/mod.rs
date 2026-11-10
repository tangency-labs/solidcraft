//! T-spline surfaces: control points on a T-mesh, which allows T-junctions, so a surface can
//! be refined locally and patches with different knot vectors join without gaps.
//!
//! References: Sederberg, Zheng, Bakenov, Nasri 2003, "T-splines and T-NURCCs" (ACM ToG
//! 22(3)) — §3 the T-spline equation and the local knot vectors read off the T-mesh by rays;
//! Sederberg, Cardon, Finnigan, North, Zheng, Lyche 2004, "T-spline simplification and local
//! refinement" (ACM ToG 23(3)) — §3 local refinement by knot insertion into blending
//! functions; Scott, Li, Sederberg, Hughes 2012, "Local refinement of analysis-suitable
//! T-splines" (CMAME 213–216) — §2 index-space T-meshes and T-junction extensions; Piegl &
//! Tiller, *The NURBS Book* — the univariate basis.
//!
//! Scope: odd degree in each direction (control points at vertices, as in the 2003 paper;
//! even degrees put anchors on faces and are refused), rectangular index domains.
//!
//! The surface is `S(s, t) = Σ wₖ Pₖ Bₖ(s, t) / Σ wₖ Bₖ(s, t)` with `Bₖ(s, t) = N[sₖ](s) ·
//! N[tₖ](t)`, the univariate B-splines of the vertex's local knot vectors (2003, eq. 2–3).
//! Near the boundary the outermost `(p−1)/2` index lines each way form a frame that must be
//! complete lines; the domain is the rectangle inside it.

mod extract;
mod mesh;
mod refine;

pub use crate::bspline::Dir;
pub use mesh::{Extension, IndexRect, MAX_GRID_POINTS, MAX_INDEX_LINES, Segment, TMesh};
pub use refine::Cut;

use crate::bspline::{HPoint, MAX_SPLINE_DEGREE, Side, SplineError, SplineResult, invalid, local_basis, weighted_points};
use crate::prelude::*;
use solidcore_record::Vec3;

/// A blending function: its anchor's local knot vectors as line indices and as values, and
/// its support.
#[derive(Clone, Debug, PartialEq)]
struct Blend {
    s: Vec<usize>,
    t: Vec<usize>,
    sv: Vec<f64>,
    tv: Vec<f64>,
}

impl Blend {
    fn support(&self) -> [[f64; 2]; 2] {
        let ends = |v: &[f64]| [v.first().copied().unwrap_or(0.0), v.last().copied().unwrap_or(0.0)];
        [ends(&self.sv), ends(&self.tv)]
    }
}

/// The number of frame lines outside the domain on each side, `(p−1)/2` per direction.
fn frame(degree: [usize; 2]) -> [usize; 2] {
    degree.map(|p| p.saturating_sub(1) / 2)
}

/// A T-spline surface of odd bi-degree.
#[derive(Clone, Debug, PartialEq)]
pub struct TSpline {
    mesh: TMesh,
    degree: [usize; 2],
    ctrl: Vec<HPoint>,
    blends: Vec<Blend>,
}

impl TSpline {
    /// A T-spline from a mesh, a bi-degree and one point and positive weight per vertex (in
    /// [`TMesh::vertices`] order).
    pub fn new(mesh: TMesh, degree: [usize; 2], points: &[Vec3], weights: &[f64]) -> SplineResult<TSpline> {
        TSpline::from_homogeneous(mesh, degree, weighted_points(points, weights)?)
    }

    /// A T-spline from homogeneous control points (`(x·w, y·w, z·w, w)`, `w > 0`).
    pub fn from_homogeneous(mesh: TMesh, degree: [usize; 2], ctrl: Vec<HPoint>) -> SplineResult<TSpline> {
        for p in degree {
            if p == 0 || p > MAX_SPLINE_DEGREE {
                return Err(SplineError::Unsupported(format!("degree {p} (1 to {MAX_SPLINE_DEGREE})")));
            }
            if p % 2 == 0 {
                return Err(SplineError::Unsupported(format!("even degree {p} (T-splines here have odd degrees)")));
            }
        }
        let g = &mesh.grid;
        let (ns, nt) = (g.ns(), g.nt());
        let [cs, ct] = frame(degree);
        if ns < 2 * cs + 2 || nt < 2 * ct + 2 {
            return invalid("too few index lines for the degree");
        }
        let full_col = |i: usize| (0..nt - 1).all(|j| g.v(i, j));
        let full_row = |j: usize| (0..ns - 1).all(|i| g.h(i, j));
        if (0..=cs).any(|k| !full_col(k) || !full_col(ns - 1 - k)) || (0..=ct).any(|k| !full_row(k) || !full_row(nt - 1 - k)) {
            return invalid("the outermost (p−1)/2 index lines must be complete");
        }
        if !(g.s.get(cs) < g.s.get(ns - 1 - cs)) || !(g.t.get(ct) < g.t.get(nt - 1 - ct)) {
            return invalid("the T-spline's domain is empty");
        }
        if ctrl.len() != mesh.vertices().len() {
            return invalid(format!("{} control points for {} vertices", ctrl.len(), mesh.vertices().len()));
        }
        if ctrl.iter().any(|h| !h.is_finite() || !(h.w > 0.0) || h.project().is_none()) {
            return invalid("control points must be finite with positive weights");
        }
        let mut blends = Vec::with_capacity(ctrl.len());
        for &(i, j) in mesh.vertices() {
            let s = g.local_knots(i, j, Dir::S, degree[0]);
            let t = g.local_knots(i, j, Dir::T, degree[1]);
            let sv: Vec<f64> = s.iter().map(|&x| g.s.get(x).copied().unwrap_or(0.0)).collect();
            let tv: Vec<f64> = t.iter().map(|&y| g.t.get(y).copied().unwrap_or(0.0)).collect();
            let b = Blend { s, t, sv, tv };
            let [[s0, s1], [t0, t1]] = b.support();
            if !(s0 < s1 && t0 < t1) {
                return invalid(format!("repeated knots leave the blending function at ({i}, {j}) empty"));
            }
            blends.push(b);
        }
        Ok(TSpline { mesh, degree, ctrl, blends })
    }

    pub fn mesh(&self) -> &TMesh {
        &self.mesh
    }
    pub fn degree(&self) -> [usize; 2] {
        self.degree
    }
    /// Homogeneous control points, one per vertex.
    pub fn homogeneous(&self) -> &[HPoint] {
        &self.ctrl
    }
    /// Cartesian control points, one per vertex.
    pub fn points(&self) -> Vec<Vec3> {
        self.ctrl.iter().map(|h| h.project().unwrap_or(Vec3::ZERO)).collect()
    }
    /// Weights, one per vertex.
    pub fn weights(&self) -> Vec<f64> {
        self.ctrl.iter().map(|h| h.w).collect()
    }
    /// The local knot vectors (values) of vertex `k`'s blending function.
    pub fn local_knots(&self, k: usize) -> Option<(&[f64], &[f64])> {
        self.blends.get(k).map(|b| (b.sv.as_slice(), b.tv.as_slice()))
    }

    /// The parameter domain `[[s₀, s₁], [t₀, t₁]]`.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        let g = &self.mesh.grid;
        let [cs, ct] = frame(self.degree);
        let at = |k: &[f64], i: usize| k.get(i).copied().unwrap_or(0.0);
        [[at(&g.s, cs), at(&g.s, g.ns() - 1 - cs)], [at(&g.t, ct), at(&g.t, g.nt() - 1 - ct)]]
    }

    /// Analysis-suitable (no s-extension meets a t-extension; Scott et al. 2012, §2).
    pub fn is_analysis_suitable(&self) -> bool {
        self.mesh.is_analysis_suitable(self.degree)
    }

    /// Which side of the knots to evaluate on, after checking `(s, t)` lies in the domain.
    fn sides(&self, s: f64, t: f64) -> SplineResult<[Side; 2]> {
        let [ds, dt] = self.domain();
        if !(s >= ds[0] && s <= ds[1] && t >= dt[0] && t <= dt[1]) {
            return invalid(format!("({s}, {t}) is outside the T-spline's domain"));
        }
        let side = |x: f64, hi: f64| if x >= hi { Side::Left } else { Side::Right };
        Ok([side(s, ds[1]), side(t, dt[1])])
    }

    /// Blending function `k` at `(s, t)`, with its first derivatives `[B, ∂B/∂s, ∂B/∂t]`.
    fn blend_at(&self, b: &Blend, s: f64, t: f64, sides: [Side; 2], n: usize) -> [f64; 3] {
        let [[s0, s1], [t0, t1]] = b.support();
        if s < s0 || s > s1 || t < t0 || t > t1 {
            return [0.0; 3];
        }
        let ns = local_basis(&b.sv, s, sides[0], n);
        let nt = local_basis(&b.tv, t, sides[1], n);
        let (n0, n1) = (ns.first().copied().unwrap_or(0.0), ns.get(1).copied().unwrap_or(0.0));
        let (m0, m1) = (nt.first().copied().unwrap_or(0.0), nt.get(1).copied().unwrap_or(0.0));
        [n0 * m0, n1 * m0, n0 * m1]
    }

    /// The value of vertex `k`'s blending function `Bₖ(s, t)` (no weight).
    pub fn blending(&self, k: usize, s: f64, t: f64) -> SplineResult<f64> {
        let sides = self.sides(s, t)?;
        let b = self.blends.get(k).ok_or_else(|| SplineError::Invalid(format!("no vertex {k}")))?;
        Ok(self.blend_at(b, s, t, sides, 0)[0])
    }

    /// Homogeneous sums `[A, ∂A/∂s, ∂A/∂t]` at `(s, t)`.
    fn sums(&self, s: f64, t: f64, n: usize) -> SplineResult<[HPoint; 3]> {
        let sides = self.sides(s, t)?;
        let mut acc = [HPoint::ZERO; 3];
        for (b, c) in self.blends.iter().zip(&self.ctrl) {
            let v = self.blend_at(b, s, t, sides, n);
            for (a, x) in acc.iter_mut().zip(v) {
                if x != 0.0 {
                    *a += *c * x;
                }
            }
        }
        Ok(acc)
    }

    /// The point at `(s, t)`.
    pub fn eval(&self, s: f64, t: f64) -> SplineResult<Vec3> {
        let [a, _, _] = self.sums(s, t, 0)?;
        a.project().ok_or_else(|| SplineError::Invalid(format!("the blending functions vanish at ({s}, {t})")))
    }

    /// The point and the first partial derivatives `[S, ∂S/∂s, ∂S/∂t]` at `(s, t)` (quotient
    /// rule on the homogeneous sums).
    pub fn derivs(&self, s: f64, t: f64) -> SplineResult<[Vec3; 3]> {
        let [a, a_s, a_t] = self.sums(s, t, 1)?;
        let p = a.project().ok_or_else(|| SplineError::Invalid(format!("the blending functions vanish at ({s}, {t})")))?;
        let d = |h: HPoint| (h.xyz() - p * h.w) / a.w;
        Ok([p, d(a_s), d(a_t)])
    }

    /// The unit normal `∂S/∂s × ∂S/∂t` at `(s, t)`, if the surface is regular there.
    pub fn normal(&self, s: f64, t: f64) -> SplineResult<Option<Vec3>> {
        let [_, ds, dt] = self.derivs(s, t)?;
        Ok(ds.cross(dt).normalized())
    }
}

#[cfg(test)]
mod tests;
