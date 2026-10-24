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

/// What a curve record is, for a message.
fn curve_kind(c: &Curve) -> &'static str {
    match c {
        Curve::Line { .. } => "a line",
        Curve::Circle { .. } => "a circle",
        Curve::Ellipse { .. } => "an ellipse",
        Curve::Polyline { .. } => "a polyline",
        Curve::Spline { .. } => "a B-spline",
        Curve::RationalSpline { .. } => "a NURBS",
        _ => "a derived curve",
    }
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

impl On {
    fn new(s: &Surface) -> Result<Self> {
        let surface = SurfaceEval::new(s)?;
        let dom = surface.domain();
        Ok(On { surface, starts: uv::Starts::default(), dom, face: None })
    }

    /// What the surface is, for a message.
    fn surface_kind(&self) -> &'static str {
        match &self.surface {
            SurfaceEval::Eval(crate::tess::EvalSurface::Plane { .. }, _) => "a plane",
            SurfaceEval::Cylinder { .. } => "a cylinder",
            SurfaceEval::Cone { .. } => "a cone",
            SurfaceEval::Sphere { .. } => "a sphere",
            SurfaceEval::Torus { .. } => "a torus",
            _ => "a spline surface",
        }
    }

    fn within(self, face: Option<[[f64; 2]; 2]>) -> Self {
        let face = face.filter(|f| f.iter().flatten().all(|x| x.is_finite()));
        On { face, ..self }
    }

    fn pcurve(&self, record: &Curve, curve: &CurveEval, range: [f64; 2], tol: f64) -> Result<Curve2> {
        if let Some(pc) = self.exact(record, curve, range, tol)?
            && self.lifts(&pc, curve, range, 0.5 * tol)
        {
            return Ok(pc);
        }
        Err(Error::Failed(format!("not supported yet: no exact pcurve of {} on {}", curve_kind(record), self.surface_kind())))
    }

    /// The closed-form pcurve where there is one (unchecked).
    fn exact(&self, record: &Curve, curve: &CurveEval, [t0, t1]: [f64; 2], tol: f64) -> Result<Option<Curve2>> {
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
                    return Ok(self.meridian(axes, &c, *radius, curve, [t0, t1]));
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
            (SurfaceEval::Eval(..), _) => self.along(curve, [t0, t1], tol),
            _ => None,
        })
    }

    /// A meridian of a sphere or a tube circle of a torus (a circle in a plane through the axis,
    /// centred on the sphere's centre or on the torus's ring): u fixed at the arc's azimuth, v its
    /// angle from the outward radial toward the axis, `α + s·t` (`s` ±1 as the circle runs).
    /// `None` for any other circle.
    fn meridian(&self, axes: &super::eval::Axes, c: &super::eval::Axes, radius: f64, curve: &CurveEval, [t0, t1]: [f64; 2]) -> Option<Curve2> {
        let ring = match &self.surface {
            SurfaceEval::Sphere { radius: r, .. } => (0.0, *r),
            SurfaceEval::Torus { major, minor, .. } => (*major, *minor),
            _ => return None,
        };
        let size = 1.0 + ring.0 + ring.1;
        if c.z.dot(axes.z).abs() > 1e-9 || (radius - ring.1).abs() > 1e-9 * size {
            return None;
        }
        let tm = 0.5 * (t0 + t1);
        let m = curve.point(tm)?;
        let [mx, my, _] = axes.local(m);
        let u0 = my.atan2(mx);
        let radial = (axes.x * mx + axes.y * my).normalized()?;
        // The circle's centre: the sphere's, or the torus's ring at that azimuth.
        let centre = axes.o + radial * ring.0;
        if c.o.dist(centre) > 1e-9 * size {
            return None;
        }
        let s = c.z.dot(radial.cross(axes.z)).signum();
        let d = m - centre;
        let vm = d.dot(axes.z).atan2(d.dot(radial));
        let alpha = vm - s * tm;
        Some(Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![[u0, alpha + s * t0], [u0, alpha + s * t1]] })
    }

    /// On a spline face (a NURBS form, or another spline), the edge's pcurve where it runs along
    /// the face's parameters at its own parameter's pace: a straight line in (u, v), between the
    /// ends' parameters, or with u (or v) the edge's parameter itself (a rim of a revolved form on
    /// the same quarter grid, whose two ends are one point on the seam). A candidate is taken only
    /// where it lifts onto the edge within the record's tolerance, as the closed forms on analytic
    /// surfaces must: the closed form the geometry has, never a fit.
    fn along(&self, curve: &CurveEval, [t0, t1]: [f64; 2], tol: f64) -> Option<Curve2> {
        let (p0, p1, pm) = (curve.point(t0)?, curve.point(t1)?, curve.point(0.5 * (t0 + t1))?);
        let size = [p0, p1, pm].iter().flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()]).fold(1.0, f64::max);
        // Within the record's tolerance, as the closed forms on analytic surfaces are, and never
        // looser than rounding.
        let exact = (0.5 * tol).max(EXACT * size);
        let line = |a: [f64; 2], b: [f64; 2]| Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![a, b] };
        let mut candidates = Vec::with_capacity(3);
        let ends = (self.project_in(p0, exact), self.project_in(p1, exact));
        if let (Some(a), Some(b)) = ends {
            candidates.push(line(a, b));
        }
        // One parameter fixed where the edge's middle is (an end on a seam or at an apex may
        // project anywhere along it), the other from the ends.
        if let (Some(a), Some(b), Some(m)) = (ends.0, ends.1, self.project_in(pm, exact)) {
            candidates.push(line([m[0], a[1]], [m[0], b[1]]));
            candidates.push(line([a[0], m[1]], [b[0], m[1]]));
        }
        // u or v moving at a steady pace in the edge's own parameter, the other fixed (or both
        // moving): a rim on the face's quarter grid, its knots scaled or turned over (a closed
        // rim's two ends are one point on the seam, so its pace is read inside it, from a quarter
        // to three quarters along, unwrapped from the middle).
        if let Some(m) = self.project_in(pm, exact) {
            let tm = 0.5 * (t0 + t1);
            let (ta, tb) = (t0 + 0.25 * (t1 - t0), t0 + 0.75 * (t1 - t0));
            let near = |t: f64| curve.point(t).and_then(|p| self.project(p, Some(m), exact));
            if let (Some(a), Some(b)) = (near(ta), near(tb)) {
                let pace = [(b[0] - a[0]) / (tb - ta), (b[1] - a[1]) / (tb - ta)];
                let at = |t: f64, moving: [bool; 2]| {
                    let d = t - tm;
                    [m[0] + if moving[0] { pace[0] * d } else { 0.0 }, m[1] + if moving[1] { pace[1] * d } else { 0.0 }]
                };
                for moving in [[true, false], [false, true], [true, true]] {
                    candidates.push(line(at(t0, moving), at(t1, moving)));
                }
            }
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

    /// The parameters of `p` within `accept` inside the face's parameter box when it has one: a
    /// spline extended past its knots, or a profile turned past where the face lies, has other
    /// preimages outside it. The global search first, then from the box's centre.
    fn project_in(&self, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        let Some(b) = self.face else { return self.project(p, None, accept) };
        let pad = |d: usize| 1e-9 * (1.0 + (b[d][1] - b[d][0]).abs());
        let inside = |q: [f64; 2]| (0..2).all(|d| q[d] >= b[d][0] - pad(d) && q[d] <= b[d][1] + pad(d));
        let centre = [0.5 * (b[0][0] + b[0][1]), 0.5 * (b[1][0] + b[1][1])];
        [None, Some(centre)].into_iter().filter_map(|near| self.project(p, near, accept)).find(|q| inside(*q))
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
