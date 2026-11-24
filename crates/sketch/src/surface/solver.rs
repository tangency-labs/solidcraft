//! The surface constraint solver: residuals in chart coordinates, a finite-difference Jacobian
//! in metric-scaled chart coordinates, a damped Gauss–Newton (Levenberg–Marquardt) iteration, and
//! a rank analysis for the remaining degrees of freedom.

use std::f64::consts::{PI, TAU};

use solidcore_record::Vec2;

use crate::linalg::{Mat, determined_vars, solve as lin_solve};
use crate::model::{ConstraintKind, CurveKind, Sketch};
use crate::surface::{SurfaceChart, SurfaceGeodesicError, SurfaceRefusal};

/// Variables in one connected component. The surface solver solves the whole sketch as one dense
/// system (it has no decomposition), so the bound is the planar solver's component bound.
const MAX_ITERS: usize = 500;
/// Converged when every residual is below this (mm).
const TOL: f64 = 1e-9;
/// A solution is accepted when every residual is below this (mm).
const ACCEPT: f64 = 1e-6;

/// Whether a surface solve satisfied every constraint it supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceSolveStatus {
    /// All supported constraints hold, and every point is inside the surface's domain.
    Solved,
    /// A constraint was violated, or a point left the surface.
    Failed,
}

/// What a surface solve found. Never extends the planar crate::SolveReport; a surface solve has
/// its own status, degrees of freedom and largest residual.
#[derive(Clone, Debug)]
pub struct SurfaceSolveReport {
    pub status: SurfaceSolveStatus,
    /// Remaining degrees of freedom (0 = fully constrained in the surface's symmetry group).
    pub dof: usize,
    pub iterations: usize,
    pub max_residual: f64,
    /// Constraints still violated after a failed solve.
    pub failing: Vec<String>,
    /// Per point: fully determined by constraints (fixed points count as determined).
    pub point_determined: Vec<bool>,
    /// Per curve: all of its defining values determined.
    pub curve_determined: Vec<bool>,
    /// A point that left the surface, if the solve failed that way.
    pub refused: Option<SurfaceRefusal>,
}

impl SurfaceSolveReport {
    /// Solved and nothing left the surface.
    #[must_use]
    pub fn solved(&self) -> bool {
        self.status == SurfaceSolveStatus::Solved && self.refused.is_none()
    }
}

/// Variable layout: two chart-coordinate variables per non-fixed point. The surface solver takes
/// no radius variables (circles are refused), so every variable is a point coordinate.
struct Layout {
    /// Per point: the first variable (x), or None when the point is held constant.
    pvar: Vec<Option<usize>>,
    /// Two variables per non-fixed point.
    n: usize,
}

fn layout(sk: &Sketch) -> Layout {
    let locked = sk.locked_points();
    let mut n = 0;
    let pvar = locked
        .iter()
        .map(|&held| {
            if held {
                None
            } else {
                let v = n;
                n += 2;
                Some(v)
            }
        })
        .collect();
    Layout { pvar, n }
}

/// The chart position of point i: from the unknowns when it varies, its stored position when it
/// is held.
fn point_pos(sk: &Sketch, lay: &Layout, x: &[f64], i: usize) -> [f64; 2] {
    match lay.pvar.get(i).copied().flatten() {
        Some(v) => [x.get(v).copied().unwrap_or(0.0), x.get(v + 1).copied().unwrap_or(0.0)],
        None => match sk.points.get(i) {
            Some(p) => [p.pos.x, p.pos.y],
            None => [0.0, 0.0],
        },
    }
}

/// The start and end point of a line curve.
fn line_ends(sk: &Sketch, l: usize) -> Option<(usize, usize)> {
    match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    }
}

/// The length of a chart tangent under the metric (E, F, G).
fn metric_norm(m: [f64; 3], a: f64, b: f64) -> f64 {
    (m[0] * a * a + 2.0 * m[1] * a * b + m[2] * b * b).max(0.0).sqrt()
}

/// The inner product of two chart tangents under the metric.
fn metric_dot(m: [f64; 3], a: [f64; 2], b: [f64; 2]) -> f64 {
    m[0] * a[0] * b[0] + m[1] * (a[0] * b[1] + a[1] * b[0]) + m[2] * a[1] * b[1]
}

/// The signed area form sqrt(EG-F^2) * (a_u b_v - a_v b_u) of two chart tangents.
fn metric_cross(m: [f64; 3], a: [f64; 2], b: [f64; 2]) -> f64 {
    let det = (m[0] * m[2] - m[1] * m[1]).max(0.0);
    det.sqrt() * (a[0] * b[1] - a[1] * b[0])
}

fn wrap_angle(a: f64) -> f64 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a <= -PI {
        a += TAU;
    }
    a
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |m, x| if x.is_finite() { m.max(x.abs()) } else { f64::INFINITY })
}

fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum()
}

/// The intrinsic length of a line: its straight chart path integrated against the surface metric,
/// not the straight chart step.
fn line_length(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart, l: usize) -> Result<f64, SurfaceGeodesicError> {
    let (a, b) = line_ends(sk, l).ok_or(SurfaceGeodesicError::OffDomain)?;
    let pa = point_pos(sk, lay, x, a);
    let pb = point_pos(sk, lay, x, b);
    let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
    let steps = 64usize;
    let mut sum = 0.0;
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let m = chart.metric(pa[0] + dx * t, pa[1] + dy * t).ok_or(SurfaceGeodesicError::OffDomain)?;
        let speed = metric_norm(m, dx, dy);
        let weight = if k == 0 || k == steps {
            1.0
        } else if k % 2 == 0 {
            2.0
        } else {
            4.0
        };
        sum += weight * speed;
    }
    Ok(sum / (3.0 * steps as f64))
}

/// The signed angle from line a to line b at their shared point, from the metric's signed area
/// form (the unsigned Metric::angle cannot tell the two sides apart).
fn angle_residual(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart, a: usize, b: usize, value: f64, flip: bool) -> f64 {
    let (Some(ea), Some(eb)) = (line_ends(sk, a), line_ends(sk, b)) else {
        return f64::INFINITY;
    };
    let shared = [ea.0, ea.1].into_iter().find(|&p| p == eb.0 || p == eb.1);
    let Some(shared) = shared else {
        return f64::INFINITY;
    };
    let other_a = if ea.0 == shared { ea.1 } else { ea.0 };
    let other_b = if eb.0 == shared { eb.1 } else { eb.0 };
    let p = point_pos(sk, lay, x, shared);
    let ta = match chart.geodesic(p, point_pos(sk, lay, x, other_a)) {
        Ok(g) => g.tan0,
        Err(_) => return f64::INFINITY,
    };
    let tb = match chart.geodesic(p, point_pos(sk, lay, x, other_b)) {
        Ok(g) => g.tan0,
        Err(_) => return f64::INFINITY,
    };
    let Some(m) = chart.metric(p[0], p[1]) else {
        return f64::INFINITY;
    };
    if !(m[0].is_finite() && m[1].is_finite() && m[2].is_finite()) {
        return f64::INFINITY;
    }
    let ang = metric_cross(m, ta, tb).atan2(metric_dot(m, ta, tb));
    let target = if flip { value + PI } else { value };
    // Scale by the characteristic length sqrt(E+2F+G), so an angular row cannot be satisfied by
    // shrinking geometry and stays comparable to a millimetre row.
    let scale = metric_norm(m, 1.0, 1.0).max(1e-6);
    wrap_angle(ang - target) * scale
}

/// The distance from point p to the drawn line c: the smallest geodesic distance from p to the
/// path's samples (so the answer is only as fine as the geodesic's sampling).
fn point_on_curve_residual(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart, p: usize, c: usize) -> f64 {
    let Some((a, b)) = line_ends(sk, c) else {
        return f64::INFINITY;
    };
    let pp = point_pos(sk, lay, x, p);
    let g = match chart.geodesic(point_pos(sk, lay, x, a), point_pos(sk, lay, x, b)) {
        Ok(g) => g,
        Err(_) => return f64::INFINITY,
    };
    let mut best = f64::INFINITY;
    for &sample in &g.samples {
        if let Ok(d) = chart.distance(pp, sample)
            && d < best
        {
            best = d;
        }
    }
    best
}

fn constraint_residual(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart, k: &ConstraintKind, out: &mut Vec<f64>) {
    use ConstraintKind::*;
    match *k {
        Coincident { p, q } => {
            let a = point_pos(sk, lay, x, p);
            let b = point_pos(sk, lay, x, q);
            out.extend([b[0] - a[0], b[1] - a[1]]);
        }
        Fix { .. } => {}
        // A distance within tolerance of zero means coincidence: two component rows keep both
        // directions constrained even when the points meet (the planar treatment).
        Distance { p, q, value } if value.abs() <= TOL => {
            let a = point_pos(sk, lay, x, p);
            let b = point_pos(sk, lay, x, q);
            out.extend([b[0] - a[0], b[1] - a[1]]);
        }
        Distance { p, q, value } => {
            let a = point_pos(sk, lay, x, p);
            let b = point_pos(sk, lay, x, q);
            out.push(match chart.distance(a, b) {
                Ok(d) => d - value,
                Err(_) => f64::INFINITY,
            });
        }
        Length { l, value } if value.abs() <= TOL => {
            if let Some((a, b)) = line_ends(sk, l) {
                let pa = point_pos(sk, lay, x, a);
                let pb = point_pos(sk, lay, x, b);
                out.extend([pb[0] - pa[0], pb[1] - pa[1]]);
            }
        }
        Length { l, value } => {
            out.push(match line_length(sk, lay, x, chart, l) {
                Ok(d) => d - value,
                Err(_) => f64::INFINITY,
            });
        }
        Angle { a, b, value, flip } => {
            out.push(angle_residual(sk, lay, x, chart, a, b, value, flip));
        }
        PointOnCurve { p, c } => {
            out.push(point_on_curve_residual(sk, lay, x, chart, p, c));
        }
        // Every other kind is refused up front by first_refusal, never reached here.
        _ => {}
    }
}

fn residuals_vec(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart) -> Vec<f64> {
    let mut out = Vec::new();
    for c in &sk.constraints {
        constraint_residual(sk, lay, x, chart, &c.kind, &mut out);
    }
    out
}

/// The metric scale sigma = sqrt(E+G) per variable, clamped to [1e-9, 1e9], re-read at the
/// current position so a stale metric never leaks into a step near a pole or seam.
fn sigmas(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart) -> Vec<f64> {
    let mut out = vec![1.0; lay.n];
    for i in 0..sk.points.len() {
        let Some(v) = lay.pvar.get(i).copied().flatten() else {
            continue;
        };
        let p = point_pos(sk, lay, x, i);
        let sigma = match chart.metric(p[0], p[1]) {
            Some(m) if m[0].is_finite() && m[2].is_finite() => (m[0] + m[2]).max(0.0).sqrt(),
            _ => 1.0,
        };
        let sigma = sigma.clamp(1e-9, 1e9);
        for k in [v, v + 1] {
            if let Some(o) = out.get_mut(k) {
                *o = sigma;
            }
        }
    }
    out
}

/// The residuals and the finite-difference Jacobian in metric-scaled chart coordinates: variable
/// v is w_v = sigma_v * x_v, so every column is a length and the rank pivot keeps its meaning.
fn system(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart) -> (Vec<f64>, Mat) {
    let r = residuals_vec(sk, lay, x, chart);
    let sigma = sigmas(sk, lay, x, chart);
    let mut j = Mat::zeros(r.len(), lay.n);
    for v in 0..lay.n {
        let s = sigma.get(v).copied().unwrap_or(1.0);
        let w = s * x.get(v).copied().unwrap_or(0.0);
        let h = 1e-7 * w.abs().max(1.0);
        let dx = h / s;
        let x0 = x.get(v).copied().unwrap_or(0.0);
        let mut plus = x.to_vec();
        let mut minus = x.to_vec();
        if let Some(p) = plus.get_mut(v) {
            *p = x0 + dx;
        }
        if let Some(m) = minus.get_mut(v) {
            *m = x0 - dx;
        }
        let rp = residuals_vec(sk, lay, &plus, chart);
        let rm = residuals_vec(sk, lay, &minus, chart);
        for row in 0..r.len() {
            let d = (rp.get(row).copied().unwrap_or(0.0) - rm.get(row).copied().unwrap_or(0.0)) / (2.0 * h);
            j.set(row, v, d);
        }
    }
    (r, j)
}

/// Levenberg–Marquardt on the whole sketch (one dense system). Returns the iterations taken and
/// the largest residual left.
fn lm(sk: &Sketch, lay: &Layout, x: &mut Vec<f64>, chart: &dyn SurfaceChart) -> (usize, f64) {
    let n = lay.n;
    let (mut r, _) = system(sk, lay, x, chart);
    let mut cost = norm2(&r);
    if n == 0 {
        return (0, max_abs(&r));
    }
    let mut lambda = 1e-3;
    for it in 0..MAX_ITERS {
        if max_abs(&r) < TOL {
            return (it, max_abs(&r));
        }
        let (_, j) = system(sk, lay, x, chart);
        let mut a = Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for row in 0..j.rows {
            let ri = r.get(row).copied().unwrap_or(0.0);
            let nz: Vec<(usize, f64)> = (0..n).map(|li| (li, j.get(row, li))).filter(|&(_, d)| d != 0.0).collect();
            for &(li, di) in &nz {
                if let Some(gi) = g.get_mut(li) {
                    *gi += di * ri;
                }
                for &(lk, dk) in &nz {
                    a.add(li, lk, di * dk);
                }
            }
        }
        let mut improved = false;
        for _ in 0..30 {
            let mut m = a.clone();
            for i in 0..n {
                m.add(i, i, lambda + 1e-12);
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let Some(step) = lin_solve(m, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let sigma = sigmas(sk, lay, x, chart);
            let mut trial = x.clone();
            for v in 0..n {
                let s = sigma.get(v).copied().unwrap_or(1.0);
                if let (Some(t), Some(st)) = (trial.get_mut(v), step.get(v)) {
                    *t += st / s;
                }
            }
            let rt = residuals_vec(sk, lay, &trial, chart);
            let ft = norm2(&rt);
            if ft.is_finite() && ft < cost {
                *x = trial;
                r = rt;
                cost = ft;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
            if lambda > 1e12 {
                break;
            }
        }
        if !improved {
            return (it, max_abs(&r));
        }
    }
    (MAX_ITERS, max_abs(&r))
}

fn initial_x(sk: &Sketch, lay: &Layout) -> Vec<f64> {
    let mut x = vec![0.0; lay.n];
    for i in 0..sk.points.len() {
        if let Some(v) = lay.pvar.get(i).copied().flatten()
            && let Some(p) = sk.points.get(i)
        {
            x[v] = p.pos.x;
            x[v + 1] = p.pos.y;
        }
    }
    x
}

/// The per-point determined flags and the Jacobian's rank on the scaled Jacobian at the solution.
/// A free point on a sphere has dof = 2 (its symmetry group is counted by the tangent-space
/// Jacobian; no Pin::Floating is subtracted because translation is not a surface isometry).
fn rank_analysis(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart) -> (Vec<bool>, usize) {
    if lay.n == 0 {
        return (vec![true; sk.points.len()], 0);
    }
    let (_, j) = system(sk, lay, x, chart);
    let (var_det, rank) = determined_vars(&j, 1e-7);
    let point_det = lay
        .pvar
        .iter()
        .map(|var| match *var {
            Some(v) => var_det.get(v).copied().unwrap_or(false) && var_det.get(v + 1).copied().unwrap_or(false),
            None => true,
        })
        .collect();
    (point_det, rank)
}

fn curve_determined(sk: &Sketch, point_det: &[bool]) -> Vec<bool> {
    sk.curves
        .iter()
        .map(|c| match c.kind {
            CurveKind::Line { a, b } => point_det.get(a).copied().unwrap_or(false) && point_det.get(b).copied().unwrap_or(false),
            _ => false,
        })
        .collect()
}

/// The ids of constraints still violated after a failed solve.
fn failing(sk: &Sketch, lay: &Layout, x: &[f64], chart: &dyn SurfaceChart) -> Vec<String> {
    let mut out = Vec::new();
    for c in &sk.constraints {
        let mut r = Vec::new();
        constraint_residual(sk, lay, x, chart, &c.kind, &mut r);
        if max_abs(&r) > ACCEPT {
            out.push(c.id.clone());
        }
    }
    out
}

/// Whether the constraint is one the surface solver supports, for the refusal words.
fn unsupported_kind(sk: &Sketch, k: &ConstraintKind) -> Option<String> {
    use ConstraintKind::*;
    match k {
        Coincident { .. } | Fix { .. } | Distance { .. } => None,
        Length { l, .. } => match sk.curves.get(*l).map(|c| &c.kind) {
            Some(CurveKind::Line { .. }) => None,
            _ => Some("Length".to_string()),
        },
        Angle { a, b, .. } => {
            let la = matches!(sk.curves.get(*a).map(|c| &c.kind), Some(CurveKind::Line { .. }));
            let lb = matches!(sk.curves.get(*b).map(|c| &c.kind), Some(CurveKind::Line { .. }));
            if la && lb { None } else { Some("Angle".to_string()) }
        }
        PointOnCurve { c, .. } => match sk.curves.get(*c).map(|cc| &cc.kind) {
            Some(CurveKind::Line { .. }) => None,
            _ => Some("PointOnCurve".to_string()),
        },
        _ => Some(k.name().to_string()),
    }
}

/// The first constraint the surface solver cannot solve, if any, with the exact "not supported on
/// a curved face" words. A refusal is a typed answer, never a silent Euclidean result.
#[must_use]
pub fn first_refusal(sk: &Sketch) -> Option<SurfaceRefusal> {
    for c in &sk.constraints {
        if let Some(kind) = unsupported_kind(sk, &c.kind) {
            return Some(SurfaceRefusal::NotSupported { kind, id: c.id.clone() });
        }
    }
    None
}

/// Solve a sketch on a curved face.
///
/// # Errors
///
/// A constraint (or a curve a supported constraint is drawn on) the solver does not support, as a
/// SurfaceRefusal::NotSupported.
pub fn surface_solve(sketch: &mut Sketch, chart: &dyn SurfaceChart) -> Result<SurfaceSolveReport, SurfaceRefusal> {
    if let Some(refusal) = first_refusal(sketch) {
        return Err(refusal);
    }
    let lay = layout(sketch);
    let mut x = initial_x(sketch, &lay);
    let (iterations, max_residual) = lm(sketch, &lay, &mut x, chart);

    // A point off the surface's domain is a refusal, never a solved sketch.
    let mut refused = None;
    for i in 0..sketch.points.len() {
        let p = point_pos(sketch, &lay, &x, i);
        if !chart.in_domain(p[0], p[1]) || chart.point(p[0], p[1]).is_none() {
            refused = Some(SurfaceRefusal::OutsideSurface { id: sketch.points.get(i).map(|p| p.id.clone()).unwrap_or_default() });
            break;
        }
    }

    let (point_determined, rank) = rank_analysis(sketch, &lay, &x, chart);
    let curve_determined = curve_determined(sketch, &point_determined);
    let dof = lay.n.saturating_sub(rank);
    let solved = max_residual < ACCEPT && refused.is_none();
    let failing = if solved { Vec::new() } else { failing(sketch, &lay, &x, chart) };

    // Geometry is written back only on success: a failed solve leaves the sketch as it was.
    if solved {
        for i in 0..sketch.points.len() {
            if lay.pvar.get(i).copied().flatten().is_some() {
                let p = point_pos(sketch, &lay, &x, i);
                if let Some(sp) = sketch.points.get_mut(i) {
                    sp.pos = Vec2::new(p[0], p[1]);
                }
            }
        }
    }

    Ok(SurfaceSolveReport {
        status: if solved { SurfaceSolveStatus::Solved } else { SurfaceSolveStatus::Failed },
        dof,
        iterations,
        max_residual,
        failing,
        point_determined,
        curve_determined,
        refused,
    })
}
