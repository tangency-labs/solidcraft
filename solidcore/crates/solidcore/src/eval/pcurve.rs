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
//! checked by lifting it. On a spline face, a straight (u, v) line is taken where it lifts onto
//! the edge within the tolerance. Anything else, and any exact one that misses, is refused
//! (`Failed`, "no exact pcurve"): nothing is fitted.
//!
//! Periodic parameters are unwrapped along each curve, and each loop's uses are moved by whole
//! periods so every use starts where the one before it ends: a seam edge used twice gets two
//! pcurves a period apart. A spline closed on itself in a direction is periodic there, one knot
//! interval per period ([`uv`]). At a pole the parameter along the collapsed direction is
//! undefined; there it is taken from the nearest sample where it is defined, so a pcurve runs
//! straight into the pole, and where two uses meet at a pole the loop runs along the pole's line.
//!
//! Origin: SolidCraft's `crates/geom/src/brep/pcurve.rs` (MIT OR Apache-2.0), moved here with its
//! history (RFC 0003, 1.5 and 1.8). It takes the kernel's evaluation types ([`super::geometry`]);
//! the structural checks of a record (counts, budgets, a use's interval in its curve's natural
//! range) are the caller's, before it converts the record (`solidcore-ops`' `pcurve`).
//!
//! Deviations: the B-spline cut and the knot span search are kept here ([`segment`],
//! [`find_span`]) as the routine had them.
use super::spline::EvalSurface;
use super::uv::{self, Domain};
use super::vec::{v3, VecExt};
use super::{Axes, Curve, Curve2, CurveEval, EvalError, Surface, SurfaceEval};
use crate::math::Vec3;

/// Why no pcurves: the records are malformed or out of bounds (`Invalid`), no pcurve within
/// the tolerance was found or the geometry is not supported (`Failed`), or the fit would exceed
/// its caps (`TooLarge`).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// The records are malformed or out of bounds.
    #[error("{0}")]
    Invalid(String),
    /// No pcurve within the tolerance was found, or the geometry is not supported.
    #[error("{0}")]
    Failed(String),
    /// The fit would exceed its caps.
    #[error("{0}")]
    TooLarge(String),
}

type Result<T> = std::result::Result<T, Error>;

impl From<EvalError> for Error {
    fn from(e: EvalError) -> Self {
        match e {
            EvalError::Invalid(_) => Error::Invalid(e.to_string()),
            EvalError::TooLarge(_) => Error::TooLarge(e.to_string()),
            EvalError::Unsupported(_)
            | EvalError::Unevaluable(_)
            | EvalError::Degenerate(_)
            | EvalError::OffRange(_) => Error::Failed(e.to_string()),
        }
    }
}

/// One oriented edge of a loop, as [`pcurves`] reads it: the edge's index (a seam edge, used
/// twice, is fitted once), its curve record and interval, whether the loop runs along it, the
/// edge record's own tolerance (its pcurve is fitted to that, never closer than the face's), and
/// the use's exact pcurve where the producer holds one (`given`, on the use's interval in its
/// curve's direction: a kernel's own UV trim mapped exactly into the record's parameters). A
/// given pcurve is taken as it is, only moved by whole periods with its loop; it is never
/// checked against the edge or replaced here (`brep::geometry::check` checks it).
#[derive(Clone, Copy, Debug)]
pub struct EdgeUse<'a> {
    /// The edge's index (a seam edge used twice is fitted once).
    pub edge: u32,
    /// The edge's curve.
    pub curve: &'a Curve,
    /// The edge's interval on its curve.
    pub interval: [f64; 2],
    /// Whether the loop runs along the edge.
    pub forward: bool,
    /// The edge's own tolerance.
    pub tolerance: f64,
    /// The use's exact pcurve where the producer holds one.
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
/// An edge's tolerance over its largest distance from a face's surface: a pcurve is accepted
/// within half its edge's tolerance ([`pcurves_within`]), as SMLib's zone tolerance is half the
/// record's, so a tolerance that covers a gap goes past twice it.
pub const SURFACE_GAP_MARGIN: f64 = 3.0;

/// For each loop of a face on `surface`, the pcurve of each of its edge uses, in order, within
/// `tolerance` mm (or the use's own edge tolerance where larger) of the edge when lifted onto
/// the surface. Loops are as the record keeps them (relative to the surface's natural normal: outer counter-clockwise, inner clockwise); each
/// pcurve runs on its use's interval in its curve's direction, whatever `forward` says.
pub fn pcurves(
    surface_record: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
) -> Result<Vec<Vec<Curve2>>> {
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
    check_size(loops, tolerance)?;
    let on = On::new(surface_record)?.within(domain);
    let each = each_pcurve(surface_record, &on, loops, tolerance)?;
    let mut out = Vec::new();
    for (l, pcs) in loops.iter().zip(each) {
        let mut fitted = Vec::new();
        for (u, pc) in l.iter().zip(pcs) {
            fitted.push((pc?, u.forward));
        }
        on.close(&mut fitted, tolerance)?;
        out.push(fitted.into_iter().map(|(c, _)| c).collect());
    }
    Ok(out)
}

/// For each loop of a face on `surface`, each use's pcurve where it has a closed form (or is
/// given), else `None`: the uses whose pcurves the face's kernel makes ([`Trims`]). Not closed
/// into loops; [`pcurves_within`] does that once every use has its pcurve.
pub fn exact_pcurves(
    surface_record: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
    domain: Option<[[f64; 2]; 2]>,
) -> Result<Vec<Vec<Option<Curve2>>>> {
    check_size(loops, tolerance)?;
    let on = On::new(surface_record)?.within(domain);
    Ok(each_pcurve(surface_record, &on, loops, tolerance)?
        .into_iter()
        .map(|l| l.into_iter().map(Result::ok).collect())
        .collect())
}

fn check_size(loops: &[Vec<EdgeUse<'_>>], tolerance: f64) -> Result<()> {
    if !(tolerance.is_finite() && tolerance > 0.0 && tolerance <= 1e3) {
        return Err(Error::Invalid(format!("pcurve tolerance {tolerance}")));
    }
    let uses = loops
        .iter()
        .map(Vec::len)
        .fold(0usize, usize::saturating_add);
    if uses > MAX_USES {
        return Err(Error::TooLarge(format!("a face with {uses} edge uses")));
    }
    Ok(())
}

/// Each use's pcurve: the given one, or its closed form; a use with neither is its `Failed`
/// error, any other error fails the face.
fn each_pcurve(
    surface_record: &Surface,
    on: &On,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
) -> Result<Vec<Vec<Result<Curve2>>>> {
    super::check::surface(surface_record).map_err(|e| Error::Invalid(e.to_string()))?;
    // A seam edge used twice is worked out once.
    let mut done: Vec<(u32, Result<Curve2>)> = Vec::new();
    let mut out = Vec::new();
    for (w, l) in loops.iter().enumerate() {
        let mut pcs = Vec::new();
        for (i, u) in l.iter().enumerate() {
            // A use's interval is finite and not empty, whether its pcurve is given or made: a
            // given pcurve is checked against it, which a NaN end would pass.
            let [t0, t1] = u.interval;
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing
            if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
                return Err(Error::Invalid(format!(
                    "loop {w} use {i}: interval [{t0}, {t1}]"
                )));
            }
            if let Some(given) = u.given {
                pcs.push(Ok(own_pcurve(given, u.interval)
                    .map_err(|e| Error::Invalid(format!("loop {w} use {i}: {e}")))?));
                continue;
            }
            if let Some((_, pc)) = done.iter().find(|(e, _)| *e == u.edge) {
                pcs.push(pc.clone());
                continue;
            }
            super::check::curve(u.curve)
                .map_err(|e| Error::Invalid(format!("loop {w} use {i}: {e}")))?;
            let c = CurveEval::new(u.curve)?;
            // An edge record that holds a looser tolerance than the face's (another kernel's
            // approximated edge) is checked against its own, up to the same cap.
            let own = if u.tolerance.is_finite() && u.tolerance <= 1e3 {
                tolerance.max(u.tolerance)
            } else {
                tolerance
            };
            let pc = on.pcurve(u.curve, &c, u.interval, own).map_err(|e| {
                let at = |t: f64| c.point(t).map(|p| [p.x, p.y, p.z]);
                let message = format!(
                    "pcurve of loop {w} use {i} ({:?} to {:?}): {e}",
                    at(u.interval[0]),
                    at(u.interval[1])
                );
                match e {
                    Error::Invalid(_) => Error::Invalid(message),
                    Error::Failed(_) => Error::Failed(message),
                    Error::TooLarge(_) => Error::TooLarge(message),
                }
            });
            if let Err(e @ (Error::Invalid(_) | Error::TooLarge(_))) = pc {
                return Err(e);
            }
            done.push((u.edge, pc.clone()));
            pcs.push(pc);
        }
        out.push(pcs);
    }
    Ok(out)
}

/// A producer's own pcurve on the use's interval (within 10⁻⁹ of its magnitude, the schema's
/// slack for a linear remap's rounding). Its structure is checked by the caller, as a record.
fn own_pcurve(c: &Curve2, [t0, t1]: [f64; 2]) -> Result<Curve2> {
    let [a, b] = c
        .interval()
        .ok_or_else(|| Error::Invalid("given pcurve: no interval".into()))?;
    let slack = 1e-9 * t0.abs().max(t1.abs()).max(1.0);
    if (a - t0).abs() > slack || (b - t1).abs() > slack {
        return Err(Error::Invalid(format!(
            "given pcurve on [{a}, {b}], its edge on [{t0}, {t1}]"
        )));
    }
    Ok(c.clone())
}

/// `c` cut exactly to `[t0, t1]` where that is less than its knot interval.
fn cut(c: Curve2, [t0, t1]: [f64; 2]) -> Result<Curve2> {
    let Some([a, b]) = c.interval() else {
        return Err(Error::Invalid("pcurve: no interval".into()));
    };
    if a == t0 && b == t1 {
        return Ok(c);
    }
    let (knots, h): (&[f64], Vec<[f64; 4]>) = match &c {
        Curve2::Spline { knots, points } => (
            knots,
            points.iter().map(|p| [p[0], p[1], 0.0, 1.0]).collect(),
        ),
        Curve2::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|p| [p[0], p[1], 0.0, p[2]])
                .collect(),
        ),
    };
    let degree = knots
        .len()
        .checked_sub(h.len() + 1)
        .ok_or_else(|| Error::Invalid("pcurve degree".into()))?;
    let (knots, h) = segment(degree, knots, &h, t0, t1)
        .map_err(|e| Error::Invalid(format!("pcurve cut to [{t0}, {t1}]: {e}")))?;
    Ok(match c {
        Curve2::Spline { .. } => Curve2::Spline {
            knots,
            points: h.iter().map(|p| [p[0], p[1]]).collect(),
        },
        Curve2::RationalSpline { .. } => Curve2::RationalSpline {
            knots,
            homogeneous: h.iter().map(|p| [p[0], p[1], p[3]]).collect(),
        },
    })
}

/// A degree-1 patch of four control points on a parallelogram (a plane as a file's B-spline
/// writes it) as that plane: the patch is affine in (u, v), so it is the plane at every
/// parameter, and its pcurves have the plane's closed forms.
fn flat_patch(s: &Surface) -> Option<Surface> {
    let Surface::Spline {
        u_knots,
        v_knots,
        points,
    } = s
    else {
        return None;
    };
    let (&[a, a1, b, b1], &[c, c1, d, d1]) = (u_knots.as_slice(), v_knots.as_slice()) else {
        return None;
    };
    let [row0, row1] = points.as_slice() else {
        return None;
    };
    let (&[p00, p01], &[p10, p11]) = (row0.as_slice(), row1.as_slice()) else {
        return None;
    };
    let [p00, p01, p10, p11] = [p00, p01, p10, p11].map(v3);
    if a != a1 || b != b1 || c != c1 || d != d1 || !(a < b && c < d) {
        return None;
    }
    let size = [p00, p01, p10, p11]
        .iter()
        .flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()])
        .fold(1.0, f64::max);
    if (p00 + p11 - p10 - p01).len() > EXACT * size {
        return None;
    }
    let (du, dv) = ((p10 - p00) * (1.0 / (b - a)), (p01 - p00) * (1.0 / (d - c)));
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing
    if !(du.cross(dv).len() > EXACT * size * size) {
        return None;
    }
    let origin = p00 - du * a - dv * c;
    Some(Surface::Plane {
        origin: [origin.x, origin.y, origin.z],
        u_axis: [du.x, du.y, du.z],
        v_axis: [dv.x, dv.y, dv.z],
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
        Curve::SurfaceIntersection { .. } => "a derived curve",
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
        let flat = flat_patch(s);
        let surface = SurfaceEval::new(flat.as_ref().unwrap_or(s))?;
        let dom = surface.domain();
        Ok(On {
            surface,
            starts: uv::Starts::default(),
            dom,
            face: None,
        })
    }

    /// What the surface is, for a message.
    fn surface_kind(&self) -> &'static str {
        match &self.surface {
            SurfaceEval::Eval(EvalSurface::Plane { .. }, _) => "a plane",
            SurfaceEval::Cylinder { .. } => "a cylinder",
            SurfaceEval::Cone { .. } => "a cone",
            SurfaceEval::Sphere { .. } => "a sphere",
            SurfaceEval::Torus { .. } => "a torus",
            SurfaceEval::Eval(..) => "a spline surface",
        }
    }

    fn within(self, face: Option<[[f64; 2]; 2]>) -> Self {
        let face = face.filter(|f| f.iter().flatten().all(|x| x.is_finite()));
        On { face, ..self }
    }

    fn pcurve(
        &self,
        record: &Curve,
        curve: &CurveEval,
        range: [f64; 2],
        tol: f64,
    ) -> Result<Curve2> {
        if let Some(pc) = self.exact(record, curve, range, tol)? {
            if self.lifts(&pc, curve, range, 0.5 * tol) {
                return Ok(pc);
            }
        }
        Err(Error::Failed(format!(
            "not supported yet: no exact pcurve of {} on {}",
            curve_kind(record),
            self.surface_kind()
        )))
    }

    /// The closed-form pcurve where there is one (unchecked).
    fn exact(
        &self,
        record: &Curve,
        curve: &CurveEval,
        [t0, t1]: [f64; 2],
        tol: f64,
    ) -> Result<Option<Curve2>> {
        let segment = |a: [f64; 2], b: [f64; 2]| Curve2::Spline {
            knots: vec![t0, t0, t1, t1],
            points: vec![a, b],
        };
        let at = |t: f64| {
            curve
                .point(t)
                .ok_or_else(|| Error::Failed(format!("the curve has no point at {t}")))
        };
        Ok(match (&self.surface, record) {
            (SurfaceEval::Eval(plane @ EvalSurface::Plane { .. }, dom), _) => {
                let map = |p: Vec3| {
                    uv::project(plane, dom, p, None, 0.0)
                        .ok_or_else(|| Error::Invalid("degenerate plane".into()))
                };
                match record {
                    // The line's own ends where the interval reaches them.
                    Curve::Line { start, end } => {
                        let end_at = |t: f64| {
                            if t == 0.0 {
                                Ok(v3(*start))
                            } else if t == 1.0 {
                                Ok(v3(*end))
                            } else {
                                at(t)
                            }
                        };
                        Some(segment(map(end_at(t0)?)?, map(end_at(t1)?)?))
                    }
                    Curve::Polyline { .. } => {
                        let mut ts = vec![t0];
                        ts.extend(
                            (t0.floor() as i64 + 1..)
                                .map(|k| k as f64)
                                .take_while(|k| *k < t1)
                                .take(MAX_SPANS),
                        );
                        ts.push(t1);
                        if ts.len() > 4096 {
                            return Ok(None);
                        }
                        let mut knots = vec![t0];
                        knots.extend(ts.iter().copied());
                        knots.push(t1);
                        Some(Curve2::Spline {
                            knots,
                            points: ts.iter().map(|t| map(at(*t)?)).collect::<Result<_>>()?,
                        })
                    }
                    // The control points through the plane's affine parameters (exact), cut to the
                    // edge's interval where it covers part of its curve (a form).
                    Curve::Spline { knots, points } => {
                        let points = points.iter().map(|p| map(v3(*p))).collect::<Result<_>>()?;
                        Some(cut(
                            Curve2::Spline {
                                knots: knots.clone(),
                                points,
                            },
                            [t0, t1],
                        )?)
                    }
                    Curve::RationalSpline { knots, homogeneous } => {
                        let homogeneous = homogeneous
                            .iter()
                            .map(|h| {
                                let [u, v] = map(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))?;
                                Ok([u * h[3], v * h[3], h[3]])
                            })
                            .collect::<Result<_>>()?;
                        Some(cut(
                            Curve2::RationalSpline {
                                knots: knots.clone(),
                                homogeneous,
                            },
                            [t0, t1],
                        )?)
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
                let c = Axes::new(frame);
                let turn = c.z.dot(axes.z);
                if (turn.abs() - 1.0).abs() > 1e-9 {
                    return Ok(self.meridian(axes, &c, *radius, curve, [t0, t1]));
                }
                let sign = turn.signum();
                let u0 = c.x.dot(axes.y).atan2(c.x.dot(axes.x));
                let h = axes.local(c.o)[2];
                let v = match &self.surface {
                    SurfaceEval::Sphere { .. } => h.atan2(*radius),
                    SurfaceEval::Torus { major, tube, .. } => {
                        (h.atan2(radius - major) - tube).rem_euclid(std::f64::consts::TAU)
                    }
                    _ => h,
                };
                Some(segment([u0 + sign * t0, v], [u0 + sign * t1, v]))
            }
            (
                SurfaceEval::Cylinder { axes, .. } | SurfaceEval::Cone { axes, .. },
                Curve::Line { .. },
            ) => {
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
    fn meridian(
        &self,
        axes: &Axes,
        c: &Axes,
        radius: f64,
        curve: &CurveEval,
        [t0, t1]: [f64; 2],
    ) -> Option<Curve2> {
        // The ring's radius, the circle's, and the tube angle at v = 0.
        let (ring, tube) = match &self.surface {
            SurfaceEval::Sphere { radius: r, .. } => ((0.0, *r), 0.0),
            SurfaceEval::Torus {
                major, minor, tube, ..
            } => ((*major, *minor), *tube),
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
        let vm = d.dot(axes.z).atan2(d.dot(radial)) - tube;
        let alpha = vm - s * tm;
        Some(Curve2::Spline {
            knots: vec![t0, t0, t1, t1],
            points: vec![[u0, alpha + s * t0], [u0, alpha + s * t1]],
        })
    }

    /// On a spline face (a NURBS form, or another spline), the edge's pcurve where it runs along
    /// the face's parameters at its own parameter's pace: a straight line in (u, v), between the
    /// ends' parameters, or with u (or v) the edge's parameter itself (a rim of a revolved form on
    /// the same quarter grid, whose two ends are one point on the seam). A candidate is taken only
    /// where it lifts onto the edge within the record's tolerance, as the closed forms on analytic
    /// surfaces must: the closed form the geometry has, never a fit.
    fn along(&self, curve: &CurveEval, [t0, t1]: [f64; 2], tol: f64) -> Option<Curve2> {
        let (p0, p1, pm) = (
            curve.point(t0)?,
            curve.point(t1)?,
            curve.point(0.5 * (t0 + t1))?,
        );
        let size = [p0, p1, pm]
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()])
            .fold(1.0, f64::max);
        // Within the record's tolerance, as the closed forms on analytic surfaces are, and never
        // looser than rounding.
        let exact = (0.5 * tol).max(EXACT * size);
        let line = |a: [f64; 2], b: [f64; 2]| Curve2::Spline {
            knots: vec![t0, t0, t1, t1],
            points: vec![a, b],
        };
        let mut candidates = Vec::new();
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
                    [
                        m[0] + if moving[0] { pace[0] * d } else { 0.0 },
                        m[1] + if moving[1] { pace[1] * d } else { 0.0 },
                    ]
                };
                for moving in [[true, false], [false, true], [true, true]] {
                    candidates.push(line(at(t0, moving), at(t1, moving)));
                }
            }
        }
        candidates
            .into_iter()
            .find(|pc| self.lifts(pc, curve, [t0, t1], exact))
    }

    /// Whether `pc` lifts onto `curve` within `target` along `range` (evenly, and inside its
    /// spans).
    fn lifts(&self, pc: &Curve2, curve: &CurveEval, [t0, t1]: [f64; 2], target: f64) -> bool {
        let knots = match pc {
            Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
        };
        // The ends exactly: `t0 + (t1 − t0)·1` may round past `t1`, where the pcurve has no point.
        let even = (0..=CHECKS).map(|k| {
            if k == CHECKS {
                t1
            } else {
                t0 + (t1 - t0) * k as f64 / CHECKS as f64
            }
        });
        let spans = knots
            .windows(2)
            .filter_map(|k| Some((*k.first()?, *k.get(1)?)))
            .filter(|(a, b)| a < b)
            .take(1 << 12)
            .map(|(a, b)| 0.5 * (a + b));
        even.chain(spans).all(|t| {
            let lifted = eval(pc, t).and_then(|q| self.surface.point(q));
            matches!((lifted, curve.point(t)), (Some(a), Some(b)) if a.dist(b) <= target)
        })
    }

    /// The parameters of `p` within `accept` inside the face's parameter box when it has one: a
    /// spline extended past its knots, or a profile turned past where the face lies, has other
    /// preimages outside it. The global search first, then from the box's centre.
    fn project_in(&self, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        let Some(b) = self.face else {
            return self.project(p, None, accept);
        };
        let pad = |r: [f64; 2]| 1e-9 * (1.0 + (r[1] - r[0]).abs());
        let inside = |q: [f64; 2]| {
            q.iter()
                .zip(b)
                .all(|(x, r)| *x >= r[0] - pad(r) && *x <= r[1] + pad(r))
        };
        let centre = [0.5 * (b[0][0] + b[0][1]), 0.5 * (b[1][0] + b[1][1])];
        [None, Some(centre)]
            .into_iter()
            .filter_map(|near| self.project(p, near, accept))
            .find(|q| inside(*q))
    }

    /// The parameters of `p` within `accept`, from `near` (and unwrapped to it) when given.
    fn project(&self, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        let mut q = self.surface.project_with(&self.starts, p, near, accept)?;
        if let Some(n) = near {
            for ((x, nd), period) in q.iter_mut().zip(n).zip(self.dom.period) {
                *x = uv::unwrap(*x, nd, period);
            }
        }
        Some(q)
    }

    /// Whether `uv` is at a pole along each direction, or within `reach` mm of one
    /// ([`SurfaceEval::pole_within`]): there the parameter is as free as at the pole itself.
    fn pole(&self, uv: [f64; 2], reach: f64) -> Result<[bool; 2]> {
        Ok(self.surface.pole_within(uv, reach)?)
    }

    /// The loop's uses moved by whole periods so each starts where the one before it ends (in
    /// loop order and direction), then all together into the surface's base period where the
    /// loop's lowest point allows. Where uses meet at a pole (within `tol`) the periodic
    /// parameter is free: there the offsets that turn the loop counter-clockwise in (u, v) (a
    /// face boundary seen from the natural normal) by more than rounding are taken, else the
    /// nearest. A seam used twice from pole to pole thus encloses its face, never nothing.
    fn close(&self, uses: &mut [(Curve2, bool)], tol: f64) -> Result<()> {
        let n = uses.len();
        let mut ends = Vec::new();
        for (c, fwd) in uses.iter() {
            let [a, b] = c
                .interval()
                .ok_or_else(|| Error::Failed("pcurve interval".into()))?;
            let (Some(s), Some(e)) = (eval(c, a), eval(c, b)) else {
                return Err(Error::Failed("pcurve ends".into()));
            };
            ends.push(if *fwd { (s, e) } else { (e, s) });
        }
        for d in 0..2 {
            let Some(period) = self
                .dom
                .period
                .get(d)
                .copied()
                .flatten()
                .filter(|p| *p > 0.0)
            else {
                continue;
            };
            // Start after a pole junction if there is one: from there continuity fixes the
            // offsets up to the next pole.
            let at_pole: Vec<bool> = ends
                .iter()
                .map(|(s, _)| Ok(self.pole(*s, tol)?.get(d).copied().unwrap_or(false)))
                .collect::<Result<_>>()?;
            let first = at_pole.iter().position(|&p| p).unwrap_or(0);
            let order: Vec<usize> = (first..n).chain(0..first).collect();
            let mut shift = vec![0.0; n];
            let mut free: Vec<usize> = Vec::new();
            let mut prev: Option<f64> = None;
            for (k, &i) in order.iter().enumerate() {
                let (Some(&(s, e)), Some(slot)) = (ends.get(i), shift.get_mut(i)) else {
                    continue;
                };
                if k > 0 && at_pole.get(i).copied().unwrap_or(false) {
                    free.push(k);
                }
                *slot = prev.map_or(0.0, |p| ((p - at(s, d)) / period).round() * period);
                prev = Some(at(e, d) + *slot);
            }
            // After each later pole one period less, none or more: the right turn, then the
            // fewest moves.
            if !free.is_empty() && free.len() <= 6 {
                // Less than this is rounding: a loop that encloses nothing (a seam's two uses
                // on one line) has no turn, whatever the sign of its area.
                let other = uses
                    .iter()
                    .flat_map(|(c, _)| points(c).into_iter().filter_map(|q| q.get(1 - d).copied()));
                let (lo, hi) = other.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                    (a.min(x), b.max(x))
                });
                let flat = if hi >= lo {
                    1e-9 * period * (hi - lo)
                } else {
                    0.0
                };
                // With the face's box: how far the loop's extent along `d` is from the box's
                // (in steps of a millionth of a period, so rounding doesn't decide).
                let width = self
                    .face
                    .and_then(|f| f.get(d).copied())
                    .map(|[a, b]| (b - a).abs());
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
                    if b < a {
                        0
                    } else {
                        ((b - a - width).abs() / period * 1e6).round() as u64
                    }
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
                    if best.as_ref().is_none_or(|(f, r, m, _)| {
                        fit < *f || (fit == *f && ((right && !r) || (right == *r && moved < *m)))
                    }) {
                        best = Some((fit, right, moved, extra));
                    }
                }
                if let Some((_, _, _, extra)) = best {
                    shift = extra;
                }
            }
            let lo = self
                .dom
                .range
                .get(d)
                .copied()
                .flatten()
                .map_or(0.0, |(a, b)| a.min(b));
            let low = uses
                .iter()
                .zip(&shift)
                .flat_map(|((c, _), s)| points(c).into_iter().map(move |q| at(q, d) + s))
                .fold(f64::INFINITY, f64::min);
            let base = if low.is_finite() {
                ((low - lo) / period + 1e-9).floor() * period
            } else {
                0.0
            };
            for (((c, _), s), (st, en)) in uses.iter_mut().zip(&shift).zip(ends.iter_mut()) {
                let by = s - base;
                if by != 0.0 {
                    move_by(c, d, by);
                    for x in [st.get_mut(d), en.get_mut(d)].into_iter().flatten() {
                        *x += by;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The `d`th coordinate of `q` (`d` is 0 or 1).
fn at(q: [f64; 2], d: usize) -> f64 {
    q.get(d).copied().unwrap_or(0.0)
}

/// Signed area in (u, v) of the loop with its uses moved by `shift` along `d`.
fn area(uses: &[(Curve2, bool)], shift: &[f64], d: usize) -> f64 {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    for ((c, fwd), s) in uses.iter().zip(shift) {
        let Some([a, b]) = c.interval() else { continue };
        for k in 0..16 {
            let f = f64::from(k) / 16.0;
            let t = if *fwd {
                a + (b - a) * f
            } else {
                b - (b - a) * f
            };
            if let Some(mut q) = eval(c, t) {
                if let Some(x) = q.get_mut(d) {
                    *x += s;
                }
                ring.push(q);
            }
        }
    }
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(p, q)| p[0] * q[1] - q[0] * p[1])
        .sum::<f64>()
        * 0.5
}

/// The control points of `c` in (u, v).
fn points(c: &Curve2) -> Vec<[f64; 2]> {
    match c {
        Curve2::Spline { points, .. } => points.clone(),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous
            .iter()
            .map(|h| [h[0] / h[2], h[1] / h[2]])
            .collect(),
    }
}

/// `c` moved by `by` along parameter `d`.
fn move_by(c: &mut Curve2, d: usize, by: f64) {
    match c {
        Curve2::Spline { points, .. } => points
            .iter_mut()
            .filter_map(|p| p.get_mut(d))
            .for_each(|x| *x += by),
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

/// The point of pcurve `c` at `t` (`None` outside its interval or for a malformed record).
pub fn eval(c: &Curve2, t: f64) -> Option<[f64; 2]> {
    let (knots, h): (&[f64], Vec<[f64; 3]>) = match c {
        Curve2::Spline { knots, points } => {
            (knots, points.iter().map(|&[u, v]| [u, v, 1.0]).collect())
        }
        Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
    };
    let p = knots.len().checked_sub(h.len() + 1)?;
    let span = find_span(knots, p, h.len(), t)?;
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

/// The `p + 1` basis functions non-zero on knot span `span` at `t`, of any degree (NURBS Book
/// A2.2).
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
            let tmp = if den == 0.0 { 0.0 } else { n.get(r)? / den };
            *n.get_mut(r)? = saved + rr * tmp;
            saved = lj * tmp;
        }
        *n.get_mut(j)? = saved;
    }
    Some(n)
}

/// Rounding allowed at the ends of a knot domain, in units of `f64::EPSILON` times its size.
const DOMAIN_ROUNDING: f64 = 8.0;
/// Most knots a cut takes.
const MAX_KNOTS: usize = 1 << 14;

/// The knot span of `u` for degree `p` and `n` control points: the last non-empty span in
/// `[p, n − 1]` that starts at or before `u` (`None` outside the domain, past rounding).
fn find_span(knots: &[f64], p: usize, n: usize, u: f64) -> Option<usize> {
    if n <= p || knots.len() != n + p + 1 || !u.is_finite() {
        return None;
    }
    let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
    let slack = DOMAIN_ROUNDING * f64::EPSILON * lo.abs().max(hi.abs()).max(1.0);
    if u < lo - slack || u > hi + slack {
        return None;
    }
    let u = u.max(lo).min(hi);
    let mut k = if u >= hi {
        n - 1
    } else {
        let below = knots.get(p..n)?.partition_point(|&x| x <= u);
        p + below.checked_sub(1)?
    };
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN knot ends no span, as `<` refuses it
    while k > p && !(knots.get(k)? < knots.get(k + 1)?) {
        k -= 1;
    }
    Some(k)
}

/// One Boehm insertion of `u` into a B-spline of degree `p` with homogeneous control points.
fn insert_knot(
    p: usize,
    knots: &[f64],
    ctrl: &[[f64; 4]],
    u: f64,
) -> std::result::Result<(Vec<f64>, Vec<[f64; 4]>), String> {
    let n = ctrl.len();
    if n <= p || knots.len() != n + p + 1 {
        return Err("knot and control point counts do not match the degree".into());
    }
    if knots.len() >= MAX_KNOTS {
        return Err("too many knots".into());
    }
    if knots.iter().any(|k| !k.is_finite())
        || knots.windows(2).any(|w| matches!(w, [a, b] if b < a))
    {
        return Err("knots must be finite and not decrease".into());
    }
    let k = find_span(knots, p, n, u).ok_or_else(|| format!("knot {u} is outside the domain"))?;
    let kn = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let pt = |i: usize| ctrl.get(i).copied().unwrap_or([0.0; 4]);
    let mut q = Vec::new();
    for i in 0..=n {
        q.push(if i + p <= k {
            pt(i)
        } else if i > k {
            pt(i - 1)
        } else {
            let den = kn(i + p) - kn(i);
            let alpha = if den > 0.0 { (u - kn(i)) / den } else { 1.0 };
            let mut r = [0.0; 4];
            for ((r, x), y) in r.iter_mut().zip(pt(i)).zip(pt(i - 1)) {
                *r = x * alpha + y * (1.0 - alpha);
            }
            r
        });
    }
    let mut nk: Vec<f64> = knots.iter().take(k + 1).copied().collect();
    nk.push(u);
    nk.extend(knots.iter().skip(k + 1));
    Ok((nk, q))
}

/// The piece of a B-spline (degree `p`, knots, homogeneous control points) on `[a, b]`, a
/// sub-interval of its domain, as a clamped B-spline whose domain is exactly `[a, b]`: Boehm
/// insertion of `a` and `b` until each has multiplicity `p` (NURBS Book §5.3), then the knots and
/// control points between them.
///
/// # Errors
/// Counts that do not match the degree, an interval outside the domain, or too many knots.
pub fn segment(
    p: usize,
    knots: &[f64],
    ctrl: &[[f64; 4]],
    a: f64,
    b: f64,
) -> std::result::Result<(Vec<f64>, Vec<[f64; 4]>), String> {
    let n = ctrl.len();
    if n <= p || knots.len() != n + p + 1 {
        return Err("knot and control point counts do not match the degree".into());
    }
    let (lo, hi) = (
        knots.get(p).copied().unwrap_or(f64::NAN),
        knots.get(n).copied().unwrap_or(f64::NAN),
    );
    if !(lo <= a && a < b && b <= hi) {
        return Err(format!(
            "[{a}, {b}] is not a piece of the domain [{lo}, {hi}]"
        ));
    }
    let (mut k, mut c) = (knots.to_vec(), ctrl.to_vec());
    for t in [a, b] {
        for _ in 0..p {
            if k.iter().filter(|&&x| x == t).count() >= p {
                break;
            }
            (k, c) = insert_knot(p, &k, &c, t)?;
        }
    }
    let j0 = k
        .iter()
        .rposition(|&x| x == a)
        .filter(|&j| j >= p)
        .ok_or("segment start")?;
    let j1 = k
        .iter()
        .position(|&x| x == b)
        .filter(|&j| j > j0)
        .ok_or("segment end")?;
    let points = c.get(j0 - p..j1).ok_or("segment control points")?.to_vec();
    let inner = k.get(j0 + 1..j1).ok_or("segment knots")?;
    let mut out = Vec::new();
    out.extend(std::iter::repeat_n(a, p + 1));
    out.extend_from_slice(inner);
    out.extend(std::iter::repeat_n(b, p + 1));
    if out.len() != points.len() + p + 1 {
        return Err("segment counts".into());
    }
    Ok((out, points))
}

/// Start a closed spline or polyline at `start` inside its natural domain `[a, b]`.
/// The result has domain `[start, start + b - a]`. Parameters before `start` move
/// forward by `b - a`; all other parameters keep their points.
///
/// Uses [`segment`]'s exact homogeneous knot insertion, then a knot-vector shift.
/// The old seam has multiplicity degree. A common homogeneous scale on the
/// second piece matches unequal seam weights without changing its geometry.
/// The seam endpoints must coincide exactly in Euclidean coordinates; a gap is
/// refused rather than closed by moving a control point.
///
/// # Errors
/// An unsupported curve, invalid spline, start outside the open domain, or size
/// beyond the evaluator's limits.
pub fn restart_curve(curve: &Curve, start: f64) -> std::result::Result<Curve, String> {
    let (knots, ctrl, rational) = match curve {
        Curve::Spline { knots, points } => (
            knots.clone(),
            points
                .iter()
                .map(|&[x, y, z]| [x, y, z, 1.0])
                .collect::<Vec<_>>(),
            false,
        ),
        Curve::RationalSpline { knots, homogeneous } => (knots.clone(), homogeneous.clone(), true),
        Curve::Polyline { points } => {
            let end = points.len().saturating_sub(1) as f64;
            let knots = std::iter::once(0.0)
                .chain((0..points.len()).map(|i| i as f64))
                .chain(std::iter::once(end))
                .collect();
            (
                knots,
                points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
                false,
            )
        }
        _ => return Err("only splines and polylines can be restarted".into()),
    };
    let p = knots
        .len()
        .checked_sub(ctrl.len() + 1)
        .ok_or("spline degree")?;
    if !(1..=super::spline::MAX_SPLINE_DEGREE).contains(&p)
        || knots.len() > MAX_KNOTS
        || knots.iter().any(|t| !t.is_finite())
        || knots.windows(2).any(|w| w.first() > w.get(1))
        || ctrl
            .iter()
            .any(|h| h.iter().any(|x| !x.is_finite()) || h.get(3).is_none_or(|w| *w <= 0.0))
    {
        return Err("invalid spline to restart".into());
    }
    let (a, b) = (
        *knots.get(p).ok_or("spline start")?,
        *knots.get(ctrl.len()).ok_or("spline end")?,
    );
    if !(a < start && start < b) {
        return Err("spline restart outside the open domain".into());
    }
    let (mut tail_knots, mut tail) = segment(p, &knots, &ctrl, start, b)?;
    let (head_knots, head) = segment(p, &knots, &ctrl, a, start)?;
    let &[tx, ty, tz, tw] = tail.last().ok_or("spline seam end")?;
    let &[hx, hy, hz, hw] = head.first().ok_or("spline seam start")?;
    let tail_end = [tx / tw, ty / tw, tz / tw];
    let head_start = [hx / hw, hy / hw, hz / hw];
    if tail_end.iter().chain(&head_start).any(|x| !x.is_finite()) || tail_end != head_start {
        return Err("spline seam endpoints do not coincide".into());
    }
    // Sharing the endpoint needs p seam knots. Scale the entire head, not only
    // its endpoint: a common projective scale preserves every rational span.
    tail_knots.pop();
    tail_knots.extend(head_knots.iter().skip(p + 1).map(|t| t + (b - a)));
    tail.extend(head.into_iter().skip(1).map(|h| h.map(|x| x * (tw / hw))));
    if tail_knots.len() > MAX_KNOTS
        || tail_knots.iter().any(|t| !t.is_finite())
        || tail_knots.windows(2).any(|w| w.first() > w.get(1))
        || tail
            .iter()
            .any(|h| h.iter().any(|x| !x.is_finite()) || h.get(3).is_none_or(|w| *w <= 0.0))
    {
        return Err("restarted spline exceeds evaluation limits".into());
    }
    let restarted = if rational {
        Curve::RationalSpline {
            knots: tail_knots,
            homogeneous: tail,
        }
    } else {
        Curve::Spline {
            knots: tail_knots,
            points: tail.into_iter().map(|[x, y, z, _]| [x, y, z]).collect(),
        }
    };
    Ok(restarted)
}

#[cfg(test)]
#[path = "pcurve_tests.rs"]
mod tests;
