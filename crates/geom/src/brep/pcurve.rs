//! Pcurves from plain records: each edge use's curve in its face's surface parameters, on the
//! edge's own interval and running in its curve's direction (docs/native-brep.md, "Pcurves"),
//! within a tolerance when lifted onto the surface. One routine for every backend: computed
//! from the 3D edge and our surface record, never taken from a kernel's own parameter space.
//!
//! Exact where the edge lies on its surface in closed form: a line, polyline or spline on a
//! plane (the plane's parameters are affine in space: the curve's knots and weights with its
//! control points mapped); a circle about the axis of a cylinder, cone, sphere or torus (v
//! constant, u the circle's angle plus a constant, or minus it where the circle turns the other
//! way); a line along a ruling of a cylinder or cone (u constant, v linear). Each exact pcurve is
//! checked by lifting it; anything else, and any exact one that misses, is a cubic B-spline fitted
//! within the tolerance, never a polyline.
//!
//! Periodic parameters are unwrapped along each curve, and each loop's uses are moved by whole
//! periods so every use starts where the one before it ends: a seam edge used twice gets two
//! pcurves a period apart. A spline closed on itself in a direction is periodic there, one knot
//! interval per period ([`uv`]). At a pole the parameter along the collapsed direction is
//! undefined; there it is taken from the nearest sample where it is defined, so a pcurve runs
//! straight into the pole, and where two uses meet at a pole the loop runs along the pole's line.
use super::eval::{CurveEval, SurfaceEval};
use super::{Budget, Curve, Curve2, Surface, curve, curve_natural, interval, surface};
use crate::Vec3;
use crate::bspline::{HPoint, segment};
use crate::tess::uv::{self, Domain};

/// Why no pcurves: the records are malformed or out of bounds (`Invalid`), no pcurve within
/// the tolerance was found or the geometry is not supported (`Failed`), or the fit would exceed
/// its caps (`TooLarge`).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
    #[error("{0}")]
    TooLarge(String),
}

type Result<T> = std::result::Result<T, Error>;

/// One oriented edge of a loop, as [`pcurves`] reads it: the edge's index (a seam edge, used
/// twice, is fitted once), its curve record and interval, whether the loop runs along it, the
/// edge record's own tolerance (its pcurve is fitted to that, never closer than the face's), and
/// the use's exact pcurve where the producer holds one (`given`, on the use's interval in its
/// curve's direction: a kernel's own UV trim mapped exactly into the record's parameters). A
/// given pcurve is taken as it is, only moved by whole periods with its loop; it is never
/// checked against the edge or replaced here (`Brep::check_geometry` checks it).
#[derive(Clone, Copy, Debug)]
pub struct EdgeUse<'a> {
    pub edge: u32,
    pub curve: &'a Curve,
    pub interval: [f64; 2],
    pub forward: bool,
    pub tolerance: f64,
    pub given: Option<&'a Curve2>,
}

/// Knot spans a fitted pcurve may have (the schema takes 4096 control points).
const MAX_SPANS: usize = 4000;
/// Refinement rounds of one fit.
const MAX_ROUNDS: usize = 40;
/// Samples per knot span when a fit starts.
const PER_SPAN: usize = 4;
/// Spans a fit starts from (more where the curve has its own knots).
const START_SPANS: usize = 4;
/// Edge uses of one face.
const MAX_USES: usize = 100_000;
/// Lift checks of an exact pcurve: evenly spaced, plus one inside each of its spans up to this.
const CHECKS: usize = 16;
/// How close, against the geometry's size, a closed-form pcurve must lift onto its edge to be
/// taken as exact (rounding, well inside any tolerance).
const EXACT: f64 = 1e-9;

/// For each loop of a face on `surface`, the pcurve of each of its edge uses, in order, within
/// `tolerance` mm (or the use's own edge tolerance where larger) of the edge when lifted onto
/// the surface. Loops are as the record keeps them (relative to the surface's natural normal: outer counter-clockwise, inner clockwise); each
/// pcurve runs on its use's interval in its curve's direction, whatever `forward` says.
pub fn pcurves(surface_record: &Surface, loops: &[Vec<EdgeUse<'_>>], tolerance: f64) -> Result<Vec<Vec<Curve2>>> {
    pcurves_within(surface_record, loops, tolerance, None)
}

/// [`pcurves`] for a face whose parameter box is known (`domain`, u then v, as its kernel
/// gives it): where two uses meet at a pole, the periodic parameter's jump there is free, and
/// the loop takes the jumps whose extent in that parameter is nearest the box's. Two meridians
/// a three-quarter turn apart bound a quarter of a sphere or three quarters of it; the loop alone
/// can't tell which, the box can.
pub fn pcurves_within(
    surface_record: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
    domain: Option<[[f64; 2]; 2]>,
) -> Result<Vec<Vec<Curve2>>> {
    if !(tolerance.is_finite() && tolerance > 0.0 && tolerance <= 1e3) {
        return Err(Error::Invalid(format!("pcurve tolerance {tolerance}")));
    }
    let uses = loops.iter().map(Vec::len).fold(0usize, usize::saturating_add);
    if uses > MAX_USES {
        return Err(Error::TooLarge(format!("a face with {uses} edge uses")));
    }
    let mut budget = Budget(0);
    surface(surface_record, 0, &mut budget).map_err(|e| Error::Invalid(e.to_string()))?;
    let on = On::new(surface_record)?.within(domain);
    // A seam edge used twice is fitted once.
    let mut done: Vec<(u32, Curve2)> = Vec::new();
    let mut out = Vec::with_capacity(loops.len());
    for (w, l) in loops.iter().enumerate() {
        let mut fitted = Vec::with_capacity(l.len());
        for (i, u) in l.iter().enumerate() {
            if let Some(given) = u.given {
                fitted.push((own_pcurve(given, u.interval, &mut budget).map_err(|e| Error::Invalid(format!("loop {w} use {i}: {e}")))?, u.forward));
                continue;
            }
            let pc = match done.iter().find(|(e, _)| *e == u.edge) {
                Some((_, pc)) => pc.clone(),
                None => {
                    curve(u.curve, 0, &mut budget).map_err(|e| Error::Invalid(format!("loop {w} use {i}: {e}")))?;
                    interval(&u.interval, curve_natural(u.curve)).map_err(|e| Error::Invalid(format!("loop {w} use {i}: {e}")))?;
                    let c = CurveEval::new(u.curve)?;
                    // An edge record that holds a looser tolerance than the face's (another
                    // kernel's approximated edge) is fitted to its own, up to the same cap.
                    let own = if u.tolerance.is_finite() && u.tolerance <= 1e3 { tolerance.max(u.tolerance) } else { tolerance };
                    let pc = on.pcurve(u.curve, &c, u.interval, own).map_err(|e| {
                        let at = |t: f64| c.point(t).map(|p| [p.x, p.y, p.z]);
                        let message = format!("pcurve of loop {w} use {i} ({:?} to {:?}): {e}", at(u.interval[0]), at(u.interval[1]));
                        match e {
                            Error::Invalid(_) => Error::Invalid(message),
                            Error::Failed(_) => Error::Failed(message),
                            Error::TooLarge(_) => Error::TooLarge(message),
                        }
                    })?;
                    done.push((u.edge, pc.clone()));
                    pc
                }
            };
            fitted.push((pc, u.forward));
        }
        on.close(&mut fitted, tolerance)?;
        out.push(fitted.into_iter().map(|(c, _)| c).collect());
    }
    Ok(out)
}

/// A producer's own pcurve, checked as a record: a valid 2D B-spline on the use's interval
/// (within 10⁻⁹ of its magnitude, the schema's slack for a linear remap's rounding).
fn own_pcurve(c: &Curve2, [t0, t1]: [f64; 2], budget: &mut Budget) -> Result<Curve2> {
    super::curve2::curve2(c, budget).map_err(|e| Error::Invalid(format!("given pcurve: {e}")))?;
    let [a, b] = c.interval().ok_or_else(|| Error::Invalid("given pcurve: no interval".into()))?;
    let slack = 1e-9 * t0.abs().max(t1.abs()).max(1.0);
    if (a - t0).abs() > slack || (b - t1).abs() > slack {
        return Err(Error::Invalid(format!("given pcurve on [{a}, {b}], its edge on [{t0}, {t1}]")));
    }
    Ok(c.clone())
}

/// `c` cut exactly to `[t0, t1]` where that is less than its knot interval.
fn cut(c: Curve2, [t0, t1]: [f64; 2]) -> Result<Curve2> {
    let Some([a, b]) = c.interval() else { return Err(Error::Invalid("pcurve: no interval".into())) };
    if a == t0 && b == t1 {
        return Ok(c);
    }
    let (knots, h): (&[f64], Vec<HPoint>) = match &c {
        Curve2::Spline { knots, points } => (knots, points.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: 1.0 }).collect()),
        Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: p[2] }).collect()),
    };
    let degree = knots.len().checked_sub(h.len() + 1).ok_or_else(|| Error::Invalid("pcurve degree".into()))?;
    let (knots, h) = segment(degree, knots, &h, t0, t1).map_err(|e| Error::Invalid(format!("pcurve cut to [{t0}, {t1}]: {e:?}")))?;
    Ok(match c {
        Curve2::Spline { .. } => Curve2::Spline { knots, points: h.iter().map(|p| [p.x, p.y]).collect() },
        Curve2::RationalSpline { .. } => Curve2::RationalSpline { knots, homogeneous: h.iter().map(|p| [p.x, p.y, p.w]).collect() },
    })
}

/// A face's surface: its evaluator and parameter box, and the grid a point's parameters on it
/// are searched from, made once for all the face's points.
struct On {
    surface: SurfaceEval,
    starts: uv::Starts,
    dom: Domain,
    /// The face's own parameter box, where its kernel gives one.
    face: Option<[[f64; 2]; 2]>,
}

/// A curve sample: its parameter, point, surface parameters (periodic ones unwrapped along the
/// curve), whether the surface is degenerate along each direction there (a pole), and the
/// curve's point halfway to the next sample once measured.
#[derive(Clone, Copy)]
struct Sample {
    t: f64,
    p: Vec3,
    uv: [f64; 2],
    pole: [bool; 2],
    mid: Option<Vec3>,
}

impl On {
    fn new(s: &Surface) -> Result<Self> {
        let surface = SurfaceEval::new(s)?;
        let dom = surface.domain();
        Ok(On { surface, starts: uv::Starts::default(), dom, face: None })
    }

    fn surface_kind(&self) -> &'static str {
        match &self.surface {
            SurfaceEval::Eval(crate::tess::EvalSurface::Plane { .. }, _) => "plane",
            SurfaceEval::Cylinder { .. } => "cylinder",
            SurfaceEval::Cone { .. } => "cone",
            SurfaceEval::Sphere { .. } => "sphere",
            SurfaceEval::Torus { .. } => "torus",
            _ => "other",
        }
    }

    fn within(self, face: Option<[[f64; 2]; 2]>) -> Self {
        let face = face.filter(|f| f.iter().flatten().all(|x| x.is_finite()));
        On { face, ..self }
    }

    fn pcurve(&self, record: &Curve, curve: &CurveEval, range: [f64; 2], tol: f64) -> Result<Curve2> {
        if let Some(pc) = self.exact(record, curve, range)?
            && self.lifts(&pc, curve, range, 0.5 * tol)
        {
            return Ok(pc);
        }
        let _ = tol;
        Err(Error::Failed(format!("no exact pcurve: {:?} on {}", std::mem::discriminant(record), self.surface_kind())))
    }

    /// The closed-form pcurve where there is one (unchecked).
    fn exact(&self, record: &Curve, curve: &CurveEval, [t0, t1]: [f64; 2]) -> Result<Option<Curve2>> {
        let segment = |a: [f64; 2], b: [f64; 2]| Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![a, b] };
        let at = |t: f64| curve.point(t).ok_or_else(|| Error::Failed(format!("the curve has no point at {t}")));
        Ok(match (&self.surface, record) {
            (SurfaceEval::Eval(plane @ crate::tess::EvalSurface::Plane { .. }, dom), _) => {
                let map = |p: Vec3| uv::project(plane, dom, p, None, 0.0).ok_or_else(|| Error::Invalid("degenerate plane".into()));
                match record {
                    // The line's own ends where the interval reaches them.
                    Curve::Line { start, end } => {
                        let end_at = |t: f64| {
                            if t == 0.0 {
                                Ok(Vec3::from(*start))
                            } else if t == 1.0 {
                                Ok(Vec3::from(*end))
                            } else {
                                at(t)
                            }
                        };
                        Some(segment(map(end_at(t0)?)?, map(end_at(t1)?)?))
                    }
                    Curve::Polyline { .. } => {
                        let mut ts = vec![t0];
                        ts.extend((t0.floor() as i64 + 1..).map(|k| k as f64).take_while(|k| *k < t1).take(MAX_SPANS));
                        ts.push(t1);
                        if ts.len() > 4096 {
                            return Ok(None);
                        }
                        let mut knots = vec![t0];
                        knots.extend(ts.iter().copied());
                        knots.push(t1);
                        Some(Curve2::Spline { knots, points: ts.iter().map(|t| map(at(*t)?)).collect::<Result<_>>()? })
                    }
                    // The control points through the plane's affine parameters (exact), cut to the
                    // edge's interval where it covers part of its curve (a form).
                    Curve::Spline { knots, points } => {
                        let points = points.iter().map(|p| map(Vec3::from(*p))).collect::<Result<_>>()?;
                        Some(cut(Curve2::Spline { knots: knots.clone(), points }, [t0, t1])?)
                    }
                    Curve::RationalSpline { knots, homogeneous } => {
                        let homogeneous = homogeneous
                            .iter()
                            .map(|h| {
                                let [u, v] = map(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))?;
                                Ok([u * h[3], v * h[3], h[3]])
                            })
                            .collect::<Result<_>>()?;
                        Some(cut(Curve2::RationalSpline { knots: knots.clone(), homogeneous }, [t0, t1])?)
                    }
                    _ => None,
                }
            }
            (
                SurfaceEval::Cylinder { axes, .. }
                | SurfaceEval::Cone { axes, .. }
                | SurfaceEval::Sphere { axes, .. }
                | SurfaceEval::Torus { axes, .. },
                Curve::Circle { frame, radius },
            ) => {
                // About the axis: u the circle's angle from where its x axis points, v fixed.
                let c = super::eval::Axes::new(frame);
                let turn = c.z.dot(axes.z);
                if (turn.abs() - 1.0).abs() > 1e-9 {
                    return Ok(None);
                }
                let sign = turn.signum();
                let u0 = c.x.dot(axes.y).atan2(c.x.dot(axes.x));
                let h = axes.local(c.o)[2];
                let v = match &self.surface {
                    SurfaceEval::Sphere { .. } => h.atan2(*radius),
                    SurfaceEval::Torus { major, .. } => {
                        let v = h.atan2(radius - major);
                        if v < 0.0 { v + std::f64::consts::TAU } else { v }
                    }
                    _ => h,
                };
                Some(segment([u0 + sign * t0, v], [u0 + sign * t1, v]))
            }
            (SurfaceEval::Cylinder { axes, .. } | SurfaceEval::Cone { axes, .. }, Curve::Line { .. }) => {
                // Along a ruling: u where the line is, v its height.
                let [a, b, _] = axes.local(at(0.5 * (t0 + t1))?);
                let height = |t: f64| at(t).map(|p| axes.local(p)[2]);
                let u = b.atan2(a);
                Some(segment([u, height(t0)?], [u, height(t1)?]))
            }
            (SurfaceEval::Eval(..), _) => self.along(curve, [t0, t1]),
            _ => None,
        })
    }

    /// On a spline face (a NURBS form, or another spline), the edge's pcurve where it runs along
    /// the face's parameters at its own parameter's pace: a straight line in (u, v), between the
    /// ends' parameters, or with u (or v) the edge's parameter itself (a rim of a revolved form on
    /// the same quarter grid, whose two ends are one point on the seam). A candidate is taken only
    /// where it lifts onto the edge to within rounding, so it is the exact pcurve; none is fitted.
    fn along(&self, curve: &CurveEval, [t0, t1]: [f64; 2]) -> Option<Curve2> {
        let (p0, p1, pm) = (curve.point(t0)?, curve.point(t1)?, curve.point(0.5 * (t0 + t1))?);
        let size = [p0, p1, pm].iter().flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()]).fold(1.0, f64::max);
        let exact = EXACT * size;
        let line = |a: [f64; 2], b: [f64; 2]| Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![a, b] };
        let mut candidates = Vec::with_capacity(3);
        if let (Some(a), Some(b)) = (self.project(p0, None, exact), self.project(p1, None, exact)) {
            candidates.push(line(a, b));
        }
        if let Some([u, v]) = self.project(pm, None, exact) {
            candidates.push(line([t0, v], [t1, v]));
            candidates.push(line([u, t0], [u, t1]));
        }
        candidates.into_iter().find(|pc| self.lifts(pc, curve, [t0, t1], exact))
    }

    /// Whether `pc` lifts onto `curve` within `target` along `range` (evenly, and inside its
    /// spans).
    fn lifts(&self, pc: &Curve2, curve: &CurveEval, [t0, t1]: [f64; 2], target: f64) -> bool {
        let knots = match pc {
            Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
        };
        let even = (0..=CHECKS).map(|k| t0 + (t1 - t0) * k as f64 / CHECKS as f64);
        let spans = knots.windows(2).filter_map(|k| Some((*k.first()?, *k.get(1)?))).filter(|(a, b)| a < b).take(1 << 12).map(|(a, b)| 0.5 * (a + b));
        even.chain(spans).all(|t| {
            let lifted = pc.eval(t).and_then(|q| self.surface.point(q));
            matches!((lifted, curve.point(t)), (Some(a), Some(b)) if a.dist(b) <= target)
        })
    }

    /// The parameters of `p` within `accept`, from `near` (and unwrapped to it) when given.
    fn project(&self, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        let mut q = self.surface.project_with(&self.starts, p, near, accept)?;
        if let Some(n) = near {
            for (d, x) in q.iter_mut().enumerate() {
                *x = uv::unwrap(*x, n[d], self.dom.period.get(d).copied().flatten());
            }
        }
        Some(q)
    }

    fn sample(&self, curve: &CurveEval, t: f64, near: Option<[f64; 2]>, accept: f64, reach: f64) -> Result<Sample> {
        let p = curve.point(t).ok_or_else(|| Error::Failed(format!("the curve has no point at {t}")))?;
        let uv = self.project(p, near, accept).ok_or_else(|| self.off(p, t, near))?;
        Ok(Sample { t, p, uv, pole: self.pole(uv, reach), mid: None })
    }

    /// Whether `uv` is at a pole along each direction, or within `reach` of one: moving that
    /// parameter by half its period (or across its range) moves the point by no more than `reach`
    /// at its speed there. There the parameter is as free as at the pole itself.
    fn pole(&self, uv: [f64; 2], reach: f64) -> [bool; 2] {
        let mut out = self.surface.pole(uv);
        let Some((_, su, sv)) = self.surface.derivs(uv[0], uv[1]) else { return out };
        for (d, speed) in [su.len(), sv.len()].into_iter().enumerate() {
            let span = match (self.dom.period.get(d).copied().flatten(), self.dom.range.get(d).copied().flatten()) {
                (Some(period), _) => 0.5 * period,
                (None, Some((a, b))) => (b - a).abs(),
                (None, None) => continue,
            };
            if let Some(flag) = out.get_mut(d) {
                *flag |= speed * span <= reach;
            }
        }
        out
    }

    /// The error for a curve point `p` (at `t`) farther than the fit allows from the surface.
    fn off(&self, p: Vec3, t: f64, near: Option<[f64; 2]>) -> Error {
        let d = self.project(p, near, f64::INFINITY).and_then(|q| self.surface.point(q)).map(|s| s.dist(p));
        Error::Failed(format!("the curve at {t} lies {} mm off the surface", d.unwrap_or(f64::NAN)))
    }

    /// A cubic B-spline in (u, v) on the curve's own parameter, through the parameters of the
    /// curve's ends, least squares through samples between; spans whose lift misses the curve
    /// by more than half the tolerance (at the samples and halfway between them) are halved and
    /// sampled twice as densely until none does.
    fn fit(&self, record: &Curve, curve: &CurveEval, [t0, t1]: [f64; 2], tol: f64) -> Result<Curve2> {
        let (target, accept) = (0.5 * tol, 0.5 * tol);
        // Samples this near a pole take their free parameter from a neighbour, which moves
        // their lift by at most this (well inside the target).
        let reach = 0.25 * tol;
        // Interior knots: where the curve's own parameterisation is less smooth (a pcurve is no
        // smoother than its curve), else even.
        let mut inner = breaks(record, Some(curve), t0, t1);
        if inner.len() + 1 > MAX_SPANS / 4 {
            return Err(Error::TooLarge(format!("a pcurve of a curve with {} knots", inner.len())));
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
                let s = self.sample(curve, t, near, accept, reach)?;
                near = Some(s.uv);
                samples.push(s);
            }
        }
        for _ in 0..MAX_ROUNDS {
            let knots: Vec<f64> = [t0; 4].into_iter().chain(inner.iter().flat_map(|&(k, m)| std::iter::repeat_n(k, m))).chain([t1; 4]).collect();
            let points = least_squares(&knots, &fill_poles(&samples)).ok_or_else(|| Error::Failed("the pcurve fit is singular".into()))?;
            let pc = Curve2::Spline { knots, points };
            let miss = |t: f64, p: Vec3| pc.eval(t).and_then(|q| self.surface.point(q)).map_or(f64::INFINITY, |s| s.dist(p));
            let spans = inner.len() + 1;
            let span_of = |t: f64| inner.partition_point(|k| k.0 <= t).min(spans - 1);
            let mut bad = vec![false; spans];
            for i in 0..samples.len() {
                let Some(s) = samples.get(i).copied() else { break };
                let mid_t = samples.get(i + 1).map(|n| 0.5 * (s.t + n.t));
                let mid = s.mid.or_else(|| mid_t.and_then(|t| curve.point(t)));
                if let Some(slot) = samples.get_mut(i) {
                    slot.mid = mid;
                }
                let between = match (mid_t, mid) {
                    (Some(t), Some(m)) => miss(t, m),
                    (Some(_), None) => f64::INFINITY,
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
                        dense.push(Sample { t, p: m, uv, pole: self.pole(uv, reach), mid: None });
                    }
                    _ => dense.push(s),
                }
            }
            samples = dense;
        }
        Err(Error::Failed(format!("no pcurve within {tol} mm in {MAX_SPANS} spans")))
    }

    /// The loop's uses moved by whole periods so each starts where the one before it ends (in
    /// loop order and direction), then all together into the surface's base period where the
    /// loop's lowest point allows. Where uses meet at a pole (within `tol`) the periodic
    /// parameter is free: there the offsets that turn the loop counter-clockwise in (u, v) (a
    /// face boundary seen from the natural normal) by more than rounding are taken, else the
    /// nearest. A seam used twice from pole to pole thus encloses its face, never nothing.
    fn close(&self, uses: &mut [(Curve2, bool)], tol: f64) -> Result<()> {
        let n = uses.len();
        let mut ends = Vec::with_capacity(n);
        for (c, fwd) in uses.iter() {
            let [a, b] = c.interval().ok_or_else(|| Error::Failed("pcurve interval".into()))?;
            let (Some(s), Some(e)) = (c.eval(a), c.eval(b)) else { return Err(Error::Failed("pcurve ends".into())) };
            ends.push(if *fwd { (s, e) } else { (e, s) });
        }
        for d in 0..2 {
            let Some(period) = self.dom.period.get(d).copied().flatten().filter(|p| *p > 0.0) else { continue };
            // Start after a pole junction if there is one: from there continuity fixes the
            // offsets up to the next pole.
            let at_pole: Vec<bool> = ends.iter().map(|(s, _)| self.pole(*s, tol).get(d).copied().unwrap_or(false)).collect();
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
                // Less than this is rounding: a loop that encloses nothing (a seam's two uses
                // on one line) has no turn, whatever the sign of its area.
                let other = uses.iter().flat_map(|(c, _)| points(c).into_iter().filter_map(|q| q.get(1 - d).copied()));
                let (lo, hi) = other.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
                let flat = if hi >= lo { 1e-9 * period * (hi - lo) } else { 0.0 };
                // With the face's box: how far the loop's extent along `d` is from the box's
                // (in steps of a millionth of a period, so rounding doesn't decide).
                let width = self.face.and_then(|f| f.get(d).copied()).map(|[a, b]| (b - a).abs());
                let misfit = |extra: &[f64]| -> u64 {
                    let Some(width) = width else { return 0 };
                    let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
                    for ((c, _), s) in uses.iter().zip(extra) {
                        for q in points(c) {
                            if let Some(x) = q.get(d) {
                                a = a.min(x + s);
                                b = b.max(x + s);
                            }
                        }
                    }
                    if b < a { 0 } else { ((b - a - width).abs() / period * 1e6).round() as u64 }
                };
                let mut best: Option<(u64, bool, usize, Vec<f64>)> = None;
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
                    let right = area(uses, &extra, d) > flat;
                    let fit = misfit(&extra);
                    if best.as_ref().is_none_or(|(f, r, m, _)| fit < *f || (fit == *f && ((right && !r) || (right == *r && moved < *m)))) {
                        best = Some((fit, right, moved, extra));
                    }
                }
                if let Some((_, _, _, extra)) = best {
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
/// vertices of a polyline or an intersection curve's leader polyline (its parameter turns there).
/// A knot that leaves a spline only continuous by its multiplicity, where the spline's
/// derivative is the same either side (`eval`, the curve's own evaluator), is once
/// differentiable and gets a pcurve with a tangent there: two pieces fitted apart would meet at
/// an angle, sharp in (u, v) where the surface's parameters run at very different speeds (a
/// thread's flank, hundreds of mm per unit along the helix and one across it).
fn breaks(curve: &Curve, eval: Option<&CurveEval>, t0: f64, t1: f64) -> Vec<(f64, usize)> {
    let spline = |knots: &[f64], count: usize| -> Vec<(f64, usize)> {
        let degree = knots.len().saturating_sub(count + 1);
        let mut out: Vec<(f64, usize)> = Vec::new();
        for &k in knots.iter().filter(|&&k| k > t0 && k < t1) {
            match out.last_mut() {
                Some(last) if last.0 == k => last.1 += 1,
                _ => out.push((k, 1)),
            }
        }
        out.into_iter()
            .map(|(k, m)| {
                let mut smooth = degree.saturating_sub(m);
                if m == degree && eval.is_some_and(|e| differentiable_at(e, k)) {
                    smooth = 1;
                }
                (k, (3 - smooth.min(2)).clamp(1, 3))
            })
            .collect()
    };
    let corners = |n: usize| (1..n.saturating_sub(1)).map(|i| (i as f64, 3)).filter(|k| k.0 > t0 && k.0 < t1).collect();
    match curve {
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => Vec::new(),
        Curve::Polyline { points } => corners(points.len()),
        Curve::Spline { knots, points } => spline(knots, points.len()),
        Curve::RationalSpline { knots, homogeneous } => spline(knots, homogeneous.len()),
        Curve::SurfaceIntersection { leader, .. } => match &**leader {
            Curve::Polyline { points } => corners(points.len()),
            other => breaks(other, None, t0, t1),
        },
    }
}

/// Relative difference of a spline's derivatives either side of a knot within which it is
/// differentiable there (rounding of derivatives made equal).
const SAME_DERIVATIVE: f64 = 1e-9;

/// Whether the spline `eval` has one derivative at its knot `k` (the same from either side).
fn differentiable_at(eval: &CurveEval, k: f64) -> bool {
    let CurveEval::Spline(c) = eval else { return false };
    let Some((l, r)) = c.sides(k) else { return false };
    let size = l.len().max(r.len());
    size > 0.0 && (l - r).len() <= SAME_DERIVATIVE * size
}

/// Signed area in (u, v) of the loop with its uses moved by `shift` along `d`.
fn area(uses: &[(Curve2, bool)], shift: &[f64], d: usize) -> f64 {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    for ((c, fwd), s) in uses.iter().zip(shift) {
        let Some([a, b]) = c.interval() else { continue };
        for k in 0..16 {
            let f = k as f64 / 16.0;
            let t = if *fwd { a + (b - a) * f } else { b - (b - a) * f };
            if let Some(mut q) = c.eval(t) {
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
        let span = crate::bspline::find_span(knots, P, n, t)?;
        let nb = crate::tess::basis_ders(knots, span, P, t)?;
        let nb = nb.first()?.get(..=P)?;
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
fn points(c: &Curve2) -> Vec<[f64; 2]> {
    match c {
        Curve2::Spline { points, .. } => points.clone(),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
    }
}

/// `c` moved by `by` along parameter `d`.
fn move_by(c: &mut Curve2, d: usize, by: f64) {
    match c {
        Curve2::Spline { points, .. } => points.iter_mut().filter_map(|p| p.get_mut(d)).for_each(|x| *x += by),
        Curve2::RationalSpline { homogeneous, .. } => {
            for h in homogeneous {
                let w = h[2];
                if let Some(x) = h.get_mut(d) {
                    *x += by * w;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pcurve_tests.rs"]
pub(super) mod tests;
