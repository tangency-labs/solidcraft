//! Pcurves for truck faces, which keep none: each edge use's curve in its face's surface
//! parameters, on the parameter of the edge's own curve (as the adapter writes it), running in
//! that curve's direction. Exact where the edge lies on its surface in closed form, otherwise a
//! 2D B-spline fitted within the tolerance.
//!
//! Periodic parameters are unwrapped along each curve, and each wire's uses are moved by whole
//! periods so every use starts where the one before it ends: a seam edge used twice gets two
//! pcurves a period apart. At a pole the parameter along the collapsed direction is undefined;
//! there it is taken from the nearest sample where it is defined, so a pcurve runs straight
//! into the pole, and where two uses meet at a pole the loop runs along the pole's line.
use super::Result;
use crate::KernelError;
use crate::body::from_p3;
use crate::tess::uv;
use mt::BoundedCurve;
use solidcraft_geom::brep as b;
use solidcraft_geom::tess::EvalSurface;
use truck_modeling as mt;

/// For each of `face`'s boundary wires, in order, the pcurve of each of its edges, in order, in
/// the (u, v) of the face's surface as truck evaluates it (`ParametricSurface::subs`), within
/// `tolerance` mm when lifted onto the surface. Wires and edges run as `face.boundaries()` lists
/// them (the order the adapter writes); each pcurve runs in its curve's own direction.
pub(super) fn pcurves(face: &mt::Face, tolerance: f64) -> Result<Vec<Vec<b::Curve2>>> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(KernelError::Invalid(format!("pcurve tolerance {tolerance}")));
    }
    crate::guard("pcurves", || {
        let surface = face.surface();
        let on = On::new(&surface);
        // A seam edge used twice is fitted once.
        let mut done: Vec<(mt::EdgeID, b::Curve2)> = Vec::new();
        let mut out = Vec::new();
        for (w, wire) in face.boundaries().iter().enumerate() {
            let mut uses = Vec::new();
            for (i, edge) in wire.edge_iter().enumerate() {
                let pc = match done.iter().find(|(id, _)| *id == edge.id()) {
                    Some((_, pc)) => pc.clone(),
                    None => {
                        let curve = edge.curve();
                        let pc = on.pcurve(&curve, tolerance).map_err(|e| {
                            let (a, b) = (edge.absolute_front().point(), edge.absolute_back().point());
                            KernelError::Failed(format!("pcurve of wire {w} edge {i} ({:?} to {:?}): {e}", [a.x, a.y, a.z], [b.x, b.y, b.z]))
                        })?;
                        done.push((edge.id(), pc.clone()));
                        pc
                    }
                };
                uses.push((pc, edge.orientation()));
            }
            on.close(&mut uses, face.orientation())?;
            out.push(uses.into_iter().map(|(c, _)| c).collect());
        }
        Ok(out)
    })
}

/// Knot spans a fitted pcurve may have (the schema takes 4096 control points).
const MAX_SPANS: usize = 4000;
/// Refinement rounds of one fit.
const MAX_ROUNDS: usize = 40;
/// Samples per knot span when a fit starts.
const PER_SPAN: usize = 4;
/// Spans a fit starts from (more where the curve has its own knots).
const START_SPANS: usize = 4;

/// A surface's parameters: the evaluator's projection where it takes the surface (it is
/// parameterised as truck parameterises it), else truck's own search.
struct On<'a> {
    surface: &'a mt::Surface,
    eval: Option<EvalSurface>,
    dom: uv::Domain,
}

/// A curve sample: its parameter, point, surface parameters (periodic ones unwrapped along the
/// curve), whether the surface is degenerate along each direction there (a pole), and the
/// curve's point halfway to the next sample once measured.
#[derive(Clone, Copy)]
struct Sample {
    t: f64,
    p: mt::Point3,
    uv: [f64; 2],
    pole: [bool; 2],
    mid: Option<mt::Point3>,
}

impl<'a> On<'a> {
    fn new(surface: &'a mt::Surface) -> Self {
        use mt::ParametricSurface;
        use std::ops::Bound::{Excluded, Included};
        let eval = crate::tess::surface::eval_surface(surface);
        let dom = match &eval {
            Some(e) => uv::domain(e),
            None => {
                let bound = |r: (std::ops::Bound<f64>, std::ops::Bound<f64>)| match r {
                    (Included(a) | Excluded(a), Included(b) | Excluded(b)) => Some((a, b)),
                    _ => None,
                };
                let (ru, rv) = surface.parameter_range();
                uv::Domain { range: [bound(ru), bound(rv)], period: [surface.u_period(), surface.v_period()] }
            }
        };
        On { surface, eval, dom }
    }

    fn pcurve(&self, curve: &mt::Curve, tol: f64) -> Result<b::Curve2> {
        match (self.surface, curve) {
            (mt::Surface::Plane(_), mt::Curve::Line(_) | mt::Curve::BSplineCurve(_) | mt::Curve::NurbsCurve(_)) => on_plane(self.surface, curve),
            _ => self.fit(curve, tol),
        }
    }

    /// The parameters of `p` within `accept`, from `near` (and unwrapped to it) when given.
    fn project(&self, p: mt::Point3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        use mt::{MetricSpace, ParametricSurface, SearchParameter};
        let mut q = match &self.eval {
            Some(e) => uv::project(e, &self.dom, from_p3(p), near, accept)?,
            None => {
                let (u, v) = self.surface.search_parameter(p, near.map(|n| (n[0], n[1])), 100)?;
                (self.surface.subs(u, v).distance(p) <= accept).then_some([u, v])?
            }
        };
        if let Some(n) = near {
            for (d, x) in q.iter_mut().enumerate() {
                *x = uv::unwrap(*x, n[d], self.dom.period.get(d).copied().flatten());
            }
        }
        Some(q)
    }

    /// Whether moving along each direction at `uv` leaves the point where it is (a pole).
    fn pole(&self, uv: [f64; 2]) -> [bool; 2] {
        use mt::{InnerSpace, ParametricSurface};
        let (su, sv) = (self.surface.uder(uv[0], uv[1]).magnitude(), self.surface.vder(uv[0], uv[1]).magnitude());
        [su <= 1e-9 * sv.max(1e-12), sv <= 1e-9 * su.max(1e-12)]
    }

    fn sample(&self, curve: &mt::Curve, t: f64, near: Option<[f64; 2]>, accept: f64) -> Result<Sample> {
        use mt::ParametricCurve;
        let p = curve.subs(t);
        let uv = self.project(p, near, accept).ok_or_else(|| self.off(p, t, near))?;
        Ok(Sample { t, p, uv, pole: self.pole(uv), mid: None })
    }

    /// The error for a curve point `p` (at `t`) farther than the fit allows from the surface.
    fn off(&self, p: mt::Point3, t: f64, near: Option<[f64; 2]>) -> KernelError {
        use mt::{MetricSpace, ParametricSurface};
        let d = self.project(p, near, f64::INFINITY).map(|q| self.surface.subs(q[0], q[1]).distance(p));
        KernelError::Failed(format!("the curve at {t} lies {} mm off the surface", d.unwrap_or(f64::NAN)))
    }

    /// A cubic B-spline in (u, v) on the curve's own parameter, through the parameters of the
    /// curve's ends, least squares through samples between; spans whose lift misses the curve
    /// by more than half the tolerance (at the samples and halfway between them) are halved and
    /// sampled twice as densely until none does.
    fn fit(&self, curve: &mt::Curve, tol: f64) -> Result<b::Curve2> {
        use mt::{MetricSpace, ParametricCurve, ParametricSurface};
        let (t0, t1) = curve.range_tuple();
        if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
            return Err(KernelError::Failed("the curve has no parameter range".into()));
        }
        let (target, accept) = (0.5 * tol, 0.5 * tol);
        // Interior knots: where the curve's own parameterisation is less smooth (a pcurve is no
        // smoother than its curve), else even.
        let mut inner = breaks(curve, t0, t1);
        if inner.len() + 1 > MAX_SPANS / 4 {
            return Err(KernelError::TooLarge(format!("a pcurve of a curve with {} knots", inner.len())));
        }
        if inner.is_empty() {
            inner = (1..START_SPANS).map(|k| (t0 + (t1 - t0) * k as f64 / START_SPANS as f64, 1)).collect();
        }
        let bounds =
            |inner: &[(f64, usize)]| -> Vec<f64> { std::iter::once(t0).chain(inner.iter().map(|k| k.0)).chain(std::iter::once(t1)).collect() };
        let mut samples: Vec<Sample> = Vec::new();
        let mut near = None;
        for w in bounds(&inner).windows(2) {
            let (a, b) = (w.first().copied().unwrap_or(t0), w.get(1).copied().unwrap_or(t1));
            for k in usize::from(!samples.is_empty())..=PER_SPAN {
                let t = if k == PER_SPAN { b } else { a + (b - a) * k as f64 / PER_SPAN as f64 };
                let s = self.sample(curve, t, near, accept)?;
                near = Some(s.uv);
                samples.push(s);
            }
        }
        for _ in 0..MAX_ROUNDS {
            let knots: Vec<f64> = [t0; 4].into_iter().chain(inner.iter().flat_map(|&(k, m)| std::iter::repeat_n(k, m))).chain([t1; 4]).collect();
            let points = least_squares(&knots, &fill_poles(&samples)).ok_or_else(|| KernelError::Failed("the pcurve fit is singular".into()))?;
            let pc = b::Curve2::Spline { knots, points };
            let miss = |t: f64, p: mt::Point3| eval(&pc, t).map_or(f64::INFINITY, |q| self.surface.subs(q[0], q[1]).distance(p));
            let spans = inner.len() + 1;
            let span_of = |t: f64| inner.partition_point(|k| k.0 <= t).min(spans - 1);
            let mut bad = vec![false; spans];
            for i in 0..samples.len() {
                let Some(s) = samples.get(i).copied() else { break };
                let mid_t = samples.get(i + 1).map(|n| 0.5 * (s.t + n.t));
                let mid = s.mid.or_else(|| mid_t.map(|t| curve.subs(t)));
                if let Some(slot) = samples.get_mut(i) {
                    slot.mid = mid;
                }
                let between = match (mid_t, mid) {
                    (Some(t), Some(m)) => miss(t, m),
                    _ => 0.0,
                };
                if !(miss(s.t, s.p) <= target && between <= target)
                    && let Some(flag) = bad.get_mut(span_of(mid_t.unwrap_or(s.t)))
                {
                    *flag = true;
                }
            }
            if !bad.contains(&true) {
                return Ok(pc);
            }
            // Halve the failing spans and sample them twice as densely.
            let split: Vec<(f64, f64)> =
                bounds(&inner).windows(2).zip(&bad).filter(|(_, b)| **b).filter_map(|(w, _)| Some((*w.first()?, *w.get(1)?))).collect();
            if inner.iter().map(|k| k.1).sum::<usize>() + split.len() + 1 > MAX_SPANS {
                break;
            }
            inner.extend(split.iter().map(|(a, b)| (0.5 * (a + b), 1)));
            inner.sort_by(|a, b| a.0.total_cmp(&b.0));
            let inside = |t: f64| split.iter().any(|&(a, b)| t >= a && t < b);
            let mut dense = Vec::with_capacity(samples.len() * 2);
            for i in 0..samples.len() {
                let Some(s) = samples.get(i).copied() else { break };
                match (inside(s.t), samples.get(i + 1), s.mid) {
                    (true, Some(n), Some(m)) => {
                        let t = 0.5 * (s.t + n.t);
                        let uv = self.project(m, Some(s.uv), accept).ok_or_else(|| self.off(m, t, Some(s.uv)))?;
                        dense.push(Sample { mid: None, ..s });
                        dense.push(Sample { t, p: m, uv, pole: self.pole(uv), mid: None });
                    }
                    _ => dense.push(s),
                }
            }
            samples = dense;
        }
        Err(KernelError::Failed(format!("no pcurve within {tol} mm in {MAX_SPANS} spans")))
    }

    /// The wire's uses moved by whole periods so each starts where the one before it ends (in
    /// wire order and direction), then all together into the surface's base period where the
    /// loop's lowest point allows. Where uses meet at a pole the periodic parameter is free:
    /// there the offsets that give the loop a face boundary's turn (counter-clockwise in (u, v)
    /// for a face along its surface's normal, clockwise against it) are taken, else the nearest.
    fn close(&self, uses: &mut [(b::Curve2, bool)], forward: bool) -> Result<()> {
        let n = uses.len();
        let mut ends = Vec::with_capacity(n);
        for (c, fwd) in uses.iter() {
            let [a, b] = c.interval().ok_or_else(|| KernelError::Failed("pcurve interval".into()))?;
            let (s, e) = (eval(c, a), eval(c, b));
            let (Some(s), Some(e)) = (s, e) else { return Err(KernelError::Failed("pcurve ends".into())) };
            ends.push(if *fwd { (s, e) } else { (e, s) });
        }
        for d in 0..2 {
            let Some(period) = self.dom.period.get(d).copied().flatten().filter(|p| *p > 0.0) else { continue };
            // Start after a pole junction if there is one: from there continuity fixes the
            // offsets up to the next pole.
            let at_pole: Vec<bool> = ends.iter().map(|(s, _)| self.pole(*s).get(d).copied().unwrap_or(false)).collect();
            let first = at_pole.iter().position(|&p| p).unwrap_or(0);
            let order: Vec<usize> = (0..n).map(|k| (first + k) % n).collect();
            let mut shift = vec![0.0; n];
            let mut free: Vec<usize> = Vec::new();
            let mut prev: Option<f64> = None;
            for (k, &i) in order.iter().enumerate() {
                let (Some(&(s, e)), Some(slot)) = (ends.get(i), shift.get_mut(i)) else { continue };
                if k > 0 && at_pole.get(i).copied().unwrap_or(false) {
                    free.push(k);
                }
                *slot = prev.map_or(0.0, |p| ((p - s[d]) / period).round() * period);
                prev = Some(e[d] + *slot);
            }
            // After each later pole one period less, none or more: the right turn, then the
            // fewest moves.
            if !free.is_empty() && free.len() <= 6 {
                let want = if forward { 1.0 } else { -1.0 };
                let mut best: Option<(bool, usize, Vec<f64>)> = None;
                for combo in 0..3usize.pow(free.len() as u32) {
                    let (mut code, mut moved, mut add) = (combo, 0, 0.0);
                    let mut extra = shift.clone();
                    for (k, &i) in order.iter().enumerate() {
                        if free.contains(&k) {
                            let step = (code % 3) as f64 - 1.0;
                            code /= 3;
                            moved += usize::from(step != 0.0);
                            add += step * period;
                        }
                        if let Some(slot) = extra.get_mut(i) {
                            *slot += add;
                        }
                    }
                    let right = area(uses, &extra, d) * want > 0.0;
                    if best.as_ref().is_none_or(|(r, m, _)| (right && !r) || (right == *r && moved < *m)) {
                        best = Some((right, moved, extra));
                    }
                }
                if let Some((_, _, extra)) = best {
                    shift = extra;
                }
            }
            let lo = self.dom.range.get(d).copied().flatten().map_or(0.0, |(a, b)| a.min(b));
            let low = uses.iter().zip(&shift).flat_map(|((c, _), s)| points(c).into_iter().map(move |q| q[d] + s)).fold(f64::INFINITY, f64::min);
            let base = if low.is_finite() { ((low - lo) / period + 1e-9).floor() * period } else { 0.0 };
            for (((c, _), s), (st, en)) in uses.iter_mut().zip(&shift).zip(ends.iter_mut()) {
                let by = s - base;
                if by != 0.0 {
                    move_by(c, d, by);
                    st[d] += by;
                    en[d] += by;
                }
            }
        }
        Ok(())
    }
}

/// The interior parameters where `curve` is less than twice continuously differentiable, with
/// the knot multiplicity a cubic needs there: a spline's knots by their multiplicity, and the
/// vertices of an intersection curve's leader polyline (its parameter turns there).
fn breaks(curve: &mt::Curve, t0: f64, t1: f64) -> Vec<(f64, usize)> {
    let spline = |degree: usize, knots: &mt::KnotVec| -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = Vec::new();
        for &k in knots.iter().filter(|&&k| k > t0 && k < t1) {
            match out.last_mut() {
                Some(last) if last.0 == k => last.1 += 1,
                _ => out.push((k, 1)),
            }
        }
        out.into_iter().map(|(k, m)| (k, (3 - degree.saturating_sub(m).min(2)).clamp(1, 3))).collect()
    };
    match curve {
        mt::Curve::Line(_) => Vec::new(),
        mt::Curve::BSplineCurve(c) => spline(c.degree(), c.knot_vec()),
        mt::Curve::NurbsCurve(c) => spline(c.degree(), c.knot_vec()),
        mt::Curve::IntersectionCurve(c) => match c.leader() {
            mt::Leader::Polyline(p) => (1..p.0.len().saturating_sub(1)).map(|i| (i as f64, 3)).filter(|k| k.0 > t0 && k.0 < t1).collect(),
            mt::Leader::BSpline(c) => spline(c.degree(), c.knot_vec()),
        },
    }
}

/// Signed area in (u, v) of the wire with its uses moved by `shift` along `d`.
fn area(uses: &[(b::Curve2, bool)], shift: &[f64], d: usize) -> f64 {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    for ((c, fwd), s) in uses.iter().zip(shift) {
        let Some([a, b]) = c.interval() else { continue };
        for k in 0..16 {
            let f = k as f64 / 16.0;
            let t = if *fwd { a + (b - a) * f } else { b - (b - a) * f };
            if let Some(mut q) = eval(c, t) {
                if let Some(x) = q.get_mut(d) {
                    *x += s;
                }
                ring.push(q);
            }
        }
    }
    let n = ring.len();
    (0..n).filter_map(|i| Some((ring.get(i)?, ring.get((i + 1) % n)?))).map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum::<f64>() * 0.5
}

/// The samples' parameters, where a pole leaves one undefined taken from the nearest sample
/// where it is defined (so the pcurve runs straight into the pole, never across to another
/// angle).
fn fill_poles(samples: &[Sample]) -> Vec<(f64, [f64; 2])> {
    let mut out: Vec<(f64, [f64; 2])> = samples.iter().map(|s| (s.t, s.uv)).collect();
    for d in 0..2 {
        // Where the surface collapses along `d`, the parameter `d` says nothing.
        let free = |s: &Sample| s.pole.get(d).copied().unwrap_or(false);
        if samples.iter().all(free) {
            continue;
        }
        for (i, s) in samples.iter().enumerate() {
            if !free(s) {
                continue;
            }
            let left = samples.get(..i).and_then(|l| l.iter().rposition(|x| !free(x)));
            let right = samples.get(i + 1..).and_then(|r| r.iter().position(|x| !free(x))).map(|k| k + i + 1);
            let dt = |k: usize| samples.get(k).map_or(f64::INFINITY, |x| (x.t - s.t).abs());
            let pick = match (left, right) {
                (Some(l), Some(r)) => Some(if dt(l) <= dt(r) { l } else { r }),
                (a, b) => a.or(b),
            };
            if let (Some(src), Some(slot)) = (pick.and_then(|k| samples.get(k)), out.get_mut(i))
                && let (Some(x), Some(v)) = (slot.1.get_mut(d), src.uv.get(d))
            {
                *x = *v;
            }
        }
    }
    out
}

/// Cubic control points on `knots` through the first and last data points, least squares
/// through the rest (banded normal equations, Cholesky).
fn least_squares(knots: &[f64], data: &[(f64, [f64; 2])]) -> Option<Vec<[f64; 2]>> {
    const P: usize = 3;
    let n = knots.len().checked_sub(P + 1)?;
    let (first, last) = (data.first()?.1, data.last()?.1);
    let m = n.checked_sub(2)?;
    // Normal equations, upper band: ata[j][k] = A(j, j + k) over the free points 1..n−1.
    let mut ata = vec![[0.0f64; P + 1]; m];
    let mut atb = vec![[0.0f64; 2]; m];
    for &(t, q) in data {
        let span = solidcraft_geom::bspline::find_span(knots, P, n, t)?;
        let nb = basis(knots, P, span, t)?;
        let mut r = q;
        for (j, w) in nb.iter().enumerate() {
            let i = span + j - P;
            let fixed = if i == 0 {
                Some(first)
            } else if i + 1 == n {
                Some(last)
            } else {
                None
            };
            if let Some(f) = fixed {
                r = [r[0] - w * f[0], r[1] - w * f[1]];
            }
        }
        for (j, wj) in nb.iter().enumerate() {
            let Some(row) = (span + j - P).checked_sub(1).filter(|&r| r < m) else { continue };
            if let Some(b) = atb.get_mut(row) {
                b[0] += wj * r[0];
                b[1] += wj * r[1];
            }
            for (k, wk) in nb.iter().enumerate().skip(j) {
                let Some(col) = (span + k - P).checked_sub(1).filter(|&c| c < m) else { continue };
                if let Some(x) = ata.get_mut(row).and_then(|a| a.get_mut(col - row)) {
                    *x += wj * wk;
                }
            }
        }
    }
    // A = Rᵀ R with R upper of the same band: r[j][k] = R(j, j + k).
    let at = |r: &[[f64; P + 1]], j: usize, k: usize| r.get(j).and_then(|x| x.get(k)).copied().unwrap_or(0.0);
    let scale = ata.iter().map(|a| a[0]).fold(0.0, f64::max);
    let mut r = vec![[0.0f64; P + 1]; m];
    for j in 0..m {
        let mut diag = at(&ata, j, 0);
        for l in 1..=P.min(j) {
            diag -= at(&r, j - l, l).powi(2);
        }
        if !(diag > 1e-14 * scale) {
            return None;
        }
        let diag = diag.sqrt();
        let mut row = [diag, 0.0, 0.0, 0.0];
        for (k, x) in row.iter_mut().enumerate().skip(1).take_while(|(k, _)| j + k < m) {
            let mut v = at(&ata, j, k);
            for l in 1..=(P - k).min(j) {
                v -= at(&r, j - l, l) * at(&r, j - l, l + k);
            }
            *x = v / diag;
        }
        *r.get_mut(j)? = row;
    }
    let mut y = vec![[0.0f64; 2]; m];
    for j in 0..m {
        let mut v = *atb.get(j)?;
        for l in 1..=P.min(j) {
            let (c, yl) = (at(&r, j - l, l), *y.get(j - l)?);
            v = [v[0] - c * yl[0], v[1] - c * yl[1]];
        }
        let d = at(&r, j, 0);
        *y.get_mut(j)? = [v[0] / d, v[1] / d];
    }
    let mut x = vec![[0.0f64; 2]; m];
    for j in (0..m).rev() {
        let mut v = *y.get(j)?;
        for k in (1..=P).take_while(|k| j + k < m) {
            let (c, xk) = (at(&r, j, k), *x.get(j + k)?);
            v = [v[0] - c * xk[0], v[1] - c * xk[1]];
        }
        let d = at(&r, j, 0);
        *x.get_mut(j)? = [v[0] / d, v[1] / d];
    }
    let out: Vec<[f64; 2]> = std::iter::once(first).chain(x).chain(std::iter::once(last)).collect();
    out.iter().flatten().all(|v| v.is_finite()).then_some(out)
}

/// The control points of `c` in (u, v).
fn points(c: &b::Curve2) -> Vec<[f64; 2]> {
    match c {
        b::Curve2::Spline { points, .. } => points.clone(),
        b::Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
    }
}

/// `c` moved by `by` along parameter `d`.
fn move_by(c: &mut b::Curve2, d: usize, by: f64) {
    match c {
        b::Curve2::Spline { points, .. } => points.iter_mut().filter_map(|p| p.get_mut(d)).for_each(|x| *x += by),
        b::Curve2::RationalSpline { homogeneous, .. } => {
            for h in homogeneous {
                let w = h[2];
                if let Some(x) = h.get_mut(d) {
                    *x += by * w;
                }
            }
        }
    }
}

/// A line or (rational) B-spline on a plane: the plane's parameters are affine in space, so the
/// pcurve is the curve's own knots and weights with its control points mapped.
fn on_plane(surface: &mt::Surface, curve: &mt::Curve) -> Result<b::Curve2> {
    let Some(plane @ EvalSurface::Plane { .. }) = crate::tess::surface::eval_surface(surface) else {
        return Err(KernelError::Failed("plane parameters".into()));
    };
    let dom = uv::domain(&plane);
    let map = |p: mt::Point3| uv::project(&plane, &dom, from_p3(p), None, 0.0).ok_or_else(|| KernelError::Failed("degenerate plane".into()));
    Ok(match curve {
        mt::Curve::Line(l) => {
            let (t0, t1) = curve.range_tuple();
            b::Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![map(l.0)?, map(l.1)?] }
        }
        mt::Curve::BSplineCurve(c) => b::Curve2::Spline {
            knots: c.knot_vec().iter().copied().collect(),
            points: c.control_points().iter().map(|p| map(*p)).collect::<Result<_>>()?,
        },
        mt::Curve::NurbsCurve(c) => b::Curve2::RationalSpline {
            knots: c.knot_vec().iter().copied().collect(),
            homogeneous: c
                .control_points()
                .iter()
                .map(|h| {
                    let [u, v] = map(mt::Point3::new(h.x / h.w, h.y / h.w, h.z / h.w))?;
                    Ok([u * h.w, v * h.w, h.w])
                })
                .collect::<Result<_>>()?,
        },
        mt::Curve::IntersectionCurve(_) => return Err(KernelError::Failed("an intersection curve has no closed-form pcurve".into())),
    })
}

/// The point of `c` at `t` (`None` outside its interval or for malformed records).
pub(super) fn eval(c: &b::Curve2, t: f64) -> Option<[f64; 2]> {
    let (knots, h): (&[f64], Vec<[f64; 3]>) = match c {
        b::Curve2::Spline { knots, points } => (knots, points.iter().map(|&[u, v]| [u, v, 1.0]).collect()),
        b::Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
    };
    let p = knots.len().checked_sub(h.len() + 1)?;
    let span = solidcraft_geom::bspline::find_span(knots, p, h.len(), t)?;
    let n = basis(knots, p, span, t)?;
    let mut s = [0.0; 3];
    for (j, nj) in n.iter().enumerate() {
        let q = h.get(span + j - p)?;
        for (a, x) in s.iter_mut().zip(q) {
            *a += nj * x;
        }
    }
    (s[2].abs() > 0.0).then(|| [s[0] / s[2], s[1] / s[2]])
}

/// The `p + 1` basis functions non-zero on knot span `span` at `t` (NURBS Book A2.2).
fn basis(knots: &[f64], p: usize, span: usize, t: f64) -> Option<Vec<f64>> {
    if span < p || span + p >= knots.len() {
        return None;
    }
    let k = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    *n.first_mut()? = 1.0;
    for j in 1..=p {
        *left.get_mut(j)? = t - k(span + 1 - j);
        *right.get_mut(j)? = k(span + j) - t;
        let mut saved = 0.0;
        for r in 0..j {
            let (rr, lj) = (*right.get(r + 1)?, *left.get(j - r)?);
            let den = rr + lj;
            let tmp = if den != 0.0 { n.get(r)? / den } else { 0.0 };
            *n.get_mut(r)? = saved + rr * tmp;
            saved = lj * tmp;
        }
        *n.get_mut(j)? = saved;
    }
    Some(n)
}
