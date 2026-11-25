//! 3D sketch curves (wires): polylines with explicit coordinates that a sketch carries besides its
//! planar geometry — included 3D edges, curves projected onto surfaces, intersection curves.
//! They are reference geometry: drawn and usable as paths, not part of profiles or the solver.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::link::{Link, LinkGeom, LinkKind, LinkSource};
use crate::model::{Result, Sketch, SketchError, in_range3, radius_in_range};

/// Most points one wire may have.
pub const MAX_WIRE_POINTS: usize = 100_000;

/// Coordinate meaning of a stored wire and its editable fit points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireSpace {
    /// Legacy drawn or detached geometry remains physically fixed in the world.
    #[default]
    World,
    /// Geometry follows the sketch's owning component placement.
    Component,
}

impl WireSpace {
    /// Coordinate semantics of a retained legacy producer, including a currently lost link.
    pub fn from_legacy_link(link: &Link) -> Self {
        match (&link.kind, &link.source) {
            (LinkKind::Include, LinkSource::Edge { .. } | LinkSource::SketchCurve { .. })
            | (_, LinkSource::OnSurface { .. } | LinkSource::Intersection { .. } | LinkSource::Iso { .. }) => Self::Component,
            _ => Self::World,
        }
    }
}

/// An exact circular arc drawn in 3D space: centre, unit normal of its plane, radius, and the
/// arc from `start` to `end` counter-clockwise about `normal` (through the wire's middle fit
/// point). All coordinates are in the wire's `space`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireArc {
    pub center: Vec3,
    pub normal: Vec3,
    pub radius: f64,
    pub start: Vec3,
    pub end: Vec3,
}

/// A control point spline drawn in 3D space: the clamped uniform B-spline of `degree` over the
/// wire's `fit` points, which are its control points. The curve touches only the first and the
/// last of them; the others pull it towards them. Fewer control points than `degree + 1` lower
/// the degree to one less than their count, as for a control spline on the sketch plane
/// ([`crate::curves::bspline_beziers`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireControl {
    pub degree: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: String,
    /// Polyline in `space` coordinates.
    pub pts: Vec<Vec3>,
    #[serde(default)]
    pub space: WireSpace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Drawn in a 3D sketch: the points the curve was drawn through (one: a point; two: a
    /// line; more: a smooth spline). `pts` is made from them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fit: Vec<Vec3>,
    /// Drawn in a 3D sketch as an exact circular arc: the analytic curve, whose `fit` points
    /// are its three defining points (start, through, end) and whose `pts` are its samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<WireArc>,
    /// Drawn in a 3D sketch as a control point spline: its `fit` points are its control points
    /// (the curve passes through the first and the last only) and its `pts` are its samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<WireControl>,
    /// A linked curve's exact geometry where its source has one (an included edge or sketch
    /// curve, an intersection curve): record curves end to end along `pts`, in `space`
    /// coordinates. Empty for a curve known only as its points (projected onto a face, iso
    /// curves). Set when the link is resolved, with `pts`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exact: Vec<WireCurve>,
}

/// One exact piece of a 3D sketch curve ([`Wire::exact`]): a record curve on its interval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCurve {
    pub curve: solidcore_record::brep::Curve,
    pub interval: [f64; 2],
}

impl WireCurve {
    /// The piece carried by the rigid motion `point` of points and `vector` of directions (a
    /// placement). `None` for an intersection curve, whose surfaces are not carried here.
    pub fn mapped(&self, point: &impl Fn(Vec3) -> Vec3, vector: &impl Fn(Vec3) -> Vec3) -> Option<WireCurve> {
        use solidcore_record::brep::{Curve, Frame};
        let a = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
        let b = |v: Vec3| [v.x, v.y, v.z];
        let p = |q: [f64; 3]| b(point(a(q)));
        let frame = |f: &Frame| Frame { origin: p(f.origin), x_axis: b(vector(a(f.x_axis))), y_axis: b(vector(a(f.y_axis))) };
        let curve = match &self.curve {
            Curve::Line { start, end } => Curve::Line { start: p(*start), end: p(*end) },
            Curve::Circle { frame: f, radius } => Curve::Circle { frame: frame(f), radius: *radius },
            Curve::Ellipse { frame: f, radii } => Curve::Ellipse { frame: frame(f), radii: *radii },
            Curve::Polyline { points } => Curve::Polyline { points: points.iter().map(|q| p(*q)).collect() },
            Curve::Spline { knots, points } => Curve::Spline { knots: knots.clone(), points: points.iter().map(|q| p(*q)).collect() },
            Curve::RationalSpline { knots, homogeneous } => Curve::RationalSpline {
                knots: knots.clone(),
                homogeneous: homogeneous
                    .iter()
                    .map(|h| {
                        let q = point(a([h[0], h[1], h[2]]) * (1.0 / h[3]));
                        [q.x * h[3], q.y * h[3], q.z * h[3], h[3]]
                    })
                    .collect(),
            },
            Curve::SurfaceIntersection { .. } => return None,
        };
        Some(WireCurve { curve, interval: self.interval })
    }
}

impl Wire {
    /// The wire carried by the rigid motion `point` of points and `vector` of directions: its
    /// points, fit points and exact curves (dropped where one cannot be carried, leaving the
    /// points).
    pub fn map(&mut self, point: impl Fn(Vec3) -> Vec3, vector: impl Fn(Vec3) -> Vec3) {
        for q in self.pts.iter_mut().chain(&mut self.fit) {
            *q = point(*q);
        }
        if let Some(arc) = &mut self.arc {
            let normal = vector(arc.normal);
            arc.center = point(arc.center);
            arc.start = point(arc.start);
            arc.end = point(arc.end);
            arc.normal = normal.normalized().unwrap_or(arc.normal);
            arc.radius = arc.center.dist(arc.start);
        }
        self.exact = self.exact.iter().map(|c| c.mapped(&point, &vector)).collect::<Option<_>>().unwrap_or_default();
    }

    /// A drawn control point spline's exact curve ([`control_curve`]); `None` for any other wire.
    pub fn control_curve(&self) -> Option<WireCurve> {
        control_curve(&self.fit, self.control?.degree)
    }

    /// The samples this drawn spline or line has when its points are `fit`: over them for a
    /// control point spline, through them otherwise.
    pub(crate) fn samples_of(&self, fit: &[Vec3]) -> Result<Vec<Vec3>> {
        match self.control {
            Some(control) => drawn_control(fit, control.degree),
            None => drawn_polyline(fit),
        }
    }
}

/// Most fit points of a drawn 3D curve.
pub const MAX_FIT_POINTS: usize = 500;

/// The polyline of a drawn 3D curve through `fit`: the point itself, the line, or the centripetal
/// Catmull–Rom spline through the points (smooth, no overshoot loops), sampled
/// ([`solidcraft_geom::catmull_rom`] has the curve itself, exactly).
pub fn fit_polyline(fit: &[Vec3]) -> Vec<Vec3> {
    solidcraft_geom::catmull_rom::samples(fit)
}

/// Fewest control points of a 3D control point spline: two would make a line.
pub const MIN_CONTROL_POINTS: usize = 3;

/// Polyline samples per knot span of a 3D control point spline.
const CONTROL_SPAN_SAMPLES: usize = 24;

/// The degree of the control point spline of `degree` over `n` control points.
fn control_degree(n: usize, degree: u8) -> usize {
    usize::from(degree).min(n.saturating_sub(1)).max(1)
}

/// The knots of the clamped uniform B-spline of degree `k` over `n` control points, on [0, 1]:
/// each end `k + 1` times, and the `n - k` spans between them of equal length.
fn control_knots(n: usize, k: usize) -> Vec<f64> {
    let spans = n.saturating_sub(k).max(1);
    let mut knots = vec![0.0; k + 1];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend(std::iter::repeat_n(1.0, k + 1));
    knots
}

/// The control point spline of `degree` over `ctrl` as its exact curve, on [0, 1]: the clamped
/// uniform B-spline ([`WireControl`]). `None` for fewer than two control points or degree 0.
pub fn control_curve(ctrl: &[Vec3], degree: u8) -> Option<WireCurve> {
    if ctrl.len() < 2 || degree == 0 {
        return None;
    }
    let knots = control_knots(ctrl.len(), control_degree(ctrl.len(), degree));
    let points = ctrl.iter().map(|p| [p.x, p.y, p.z]).collect();
    Some(WireCurve { curve: solidcore_record::brep::Curve::Spline { knots, points }, interval: [0.0, 1.0] })
}

/// The polyline of the control point spline of `degree` over `ctrl`: each knot span as its
/// Bézier piece ([`solidcraft_geom::bspline::segment`], the pieces a sweep follows), sampled
/// evenly, from the first control point to the last. Empty when the spline does not evaluate.
pub fn control_polyline(ctrl: &[Vec3], degree: u8) -> Vec<Vec3> {
    control_samples(ctrl, degree).unwrap_or_default()
}

fn control_samples(ctrl: &[Vec3], degree: u8) -> Option<Vec<Vec3>> {
    use solidcraft_geom::bspline::{HPoint, bezier_point, segment};
    let (first, last) = (*ctrl.first()?, *ctrl.last()?);
    if ctrl.len() < 2 || degree == 0 {
        return None;
    }
    let k = control_degree(ctrl.len(), degree);
    let knots = control_knots(ctrl.len(), k);
    let h: Vec<HPoint> = ctrl.iter().map(|p| HPoint::weighted(*p, 1.0)).collect();
    let mut breaks = knots.clone();
    breaks.dedup();
    let mut out = Vec::with_capacity(breaks.len() * CONTROL_SPAN_SAMPLES + 1);
    out.push(first);
    for span in breaks.windows(2) {
        let (a, b) = (*span.first()?, *span.get(1)?);
        let (_, piece) = segment(k, &knots, &h, a, b).ok()?;
        for i in 1..=CONTROL_SPAN_SAMPLES {
            out.push(bezier_point(&piece, i as f64 / CONTROL_SPAN_SAMPLES as f64).project()?);
        }
    }
    // The curve ends on its last control point: the same point, not one rounded beside it.
    *out.last_mut()? = last;
    Some(out)
}

/// Chord-height tolerance (mm) when sampling a drawn 3D arc into `pts`.
const ARC_TOL: f64 = 1e-3;

/// The circular arc through `start`, `through` and `end` — the three points uniquely define
/// it — running counter-clockwise about the plane normal from `start` through `through` to
/// `end`. `None` when the points coincide, are collinear (no circle through them) or the
/// circle is out of range.
pub fn arc_from_3_points(start: Vec3, through: Vec3, end: Vec3) -> Option<WireArc> {
    let (b, c) = (through - start, end - start);
    if b.len2() < 1e-18 || c.len2() < 1e-18 {
        return None;
    }
    let n = b.cross(c);
    // Collinear points make |n| vanish relative to the chord lengths; scale the test with them.
    if n.len2() < 1e-18 * b.len2() * c.len2() {
        return None;
    }
    let normal = n.normalized()?;
    let center = start + (n.cross(b) * c.len2() + c.cross(n) * b.len2()) * (0.5 / n.len2());
    let radius = center.dist(start);
    if !center.is_finite() || !radius_in_range(radius) {
        return None;
    }
    Some(WireArc { center, normal, radius, start, end })
}

/// The sweep of an exact 3D arc: the angle in (0, 2π) from `start` to `end` counter-clockwise
/// about `normal`.
pub fn arc_sweep(arc: &WireArc) -> f64 {
    let (u, v) = (arc.start - arc.center, arc.end - arc.center);
    let a = u.cross(v).dot(arc.normal).atan2(u.dot(v));
    if a <= 0.0 { a + std::f64::consts::TAU } else { a }
}

/// The polyline of an exact 3D arc, sampled so every chord's sagitta stays within `tol`.
pub fn arc_polyline(arc: &WireArc, tol: f64) -> Vec<Vec3> {
    let sweep = arc_sweep(arc);
    if !(arc.radius > 0.0) || !arc.radius.is_finite() || !sweep.is_finite() {
        return vec![arc.start, arc.end];
    }
    // Sagitta of a chord spanning Δ on a circle r: r(1 − cos(Δ/2)) ≤ tol.
    let max_step = (2.0 * (1.0 - (tol / arc.radius).clamp(0.0, 1.0)).acos()).max(1e-4);
    let n = (sweep / max_step).ceil() as usize;
    let n = n.clamp(2, MAX_WIRE_POINTS);
    let u = arc.start - arc.center;
    let e = arc.normal.cross(u); // in the arc's plane, perpendicular to u, same length
    let mut out = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let t = sweep * k as f64 / n as f64;
        out.push(if k == n { arc.end } else { arc.center + u * t.cos() + e * t.sin() });
    }
    out
}

/// Is a saved arc consistent: in range, unit normal, and its ends on the circle?
fn arc_ok(arc: &WireArc, fit: &[Vec3]) -> bool {
    let on_circle = |p: Vec3| (arc.center.dist(p) - arc.radius).abs() < 1e-6 * arc.radius.max(1.0);
    in_range3(arc.center)
        && in_range3(arc.start)
        && in_range3(arc.end)
        && arc.normal.is_finite()
        && (arc.normal.len() - 1.0).abs() < 1e-6
        && radius_in_range(arc.radius)
        && fit.len() == 3
        && on_circle(arc.start)
        && on_circle(arc.end)
}

/// Is a saved control point spline consistent: a drawn curve that is nothing else, with a
/// degree and a count of control points the spline can have?
fn control_ok(control: &WireControl, w: &Wire) -> bool {
    control_degree_ok(control.degree)
        && (MIN_CONTROL_POINTS..=MAX_FIT_POINTS).contains(&w.fit.len())
        && w.link.is_none()
        && w.arc.is_none()
        && w.exact.is_empty()
}

fn clean(w: &[Vec3]) -> Option<Vec<Vec3>> {
    if w.len() > MAX_WIRE_POINTS {
        return None;
    }
    let mut out: Vec<Vec3> = Vec::new();
    for p in w {
        if !in_range3(*p) {
            return None;
        }
        if out.last().is_none_or(|l| l.dist(*p) > 1e-9) {
            out.push(*p);
        }
    }
    (out.len() >= 2).then_some(out)
}

fn clean_wires(wires: &[Vec<Vec3>]) -> Result<Vec<Vec<Vec3>>> {
    if wires.len() > crate::link::MAX_LINK_ENTITIES || wires.iter().fold(0usize, |n, w| n.saturating_add(w.len())) > 1_000_000 {
        return Err(SketchError::Invalid("3D linked curves exceed the stored geometry budget".into()));
    }
    wires.iter().map(|w| clean(w).ok_or_else(|| SketchError::Invalid("3D linked curve points out of range or coincident".into()))).collect()
}

/// The polyline of a drawn 3D curve through `fit`: checked for the count, the range and repeats,
/// then made by [`fit_polyline`].
pub(crate) fn drawn_polyline(fit: &[Vec3]) -> Result<Vec<Vec3>> {
    if fit.is_empty() || fit.len() > MAX_FIT_POINTS {
        return Err(SketchError::Invalid(format!("a 3D curve needs 1 to {MAX_FIT_POINTS} points")));
    }
    if fit.iter().any(|p| !in_range3(*p)) {
        return Err(SketchError::Invalid("3D point out of range".into()));
    }
    if fit.windows(2).any(|w| w[0].dist(w[1]) < 1e-9) {
        return Err(SketchError::Invalid("the points of a 3D curve must differ".into()));
    }
    let pts = fit_polyline(fit);
    if pts.iter().any(|p| !in_range3(*p)) {
        return Err(SketchError::Invalid("3D spline samples out of range".into()));
    }
    Ok(pts)
}

/// Whether a 3D control point spline may have this degree.
fn control_degree_ok(degree: u8) -> bool {
    (1..=solidcore_record::MAX_BEZIER_DEGREE).contains(&usize::from(degree))
}

/// The samples of a 3D control point spline of `degree` over `ctrl`, the control points checked
/// first.
pub(crate) fn drawn_control(ctrl: &[Vec3], degree: u8) -> Result<Vec<Vec3>> {
    if !(MIN_CONTROL_POINTS..=MAX_FIT_POINTS).contains(&ctrl.len()) {
        return Err(SketchError::Invalid(format!("a 3D control point spline needs {MIN_CONTROL_POINTS} to {MAX_FIT_POINTS} control points")));
    }
    if !control_degree_ok(degree) {
        return Err(SketchError::Invalid(format!("a 3D control point spline has degree 1 to {}", solidcore_record::MAX_BEZIER_DEGREE)));
    }
    if ctrl.iter().any(|p| !in_range3(*p)) {
        return Err(SketchError::Invalid("3D point out of range".into()));
    }
    if ctrl.windows(2).any(|w| w[0].dist(w[1]) < 1e-9) {
        return Err(SketchError::Invalid("neighbouring control points of a 3D curve must differ".into()));
    }
    control_samples(ctrl, degree)
        .filter(|pts| pts.iter().all(|p| in_range3(*p)))
        .ok_or_else(|| SketchError::Invalid("3D spline samples out of range".into()))
}

/// Refuse a sketch whose 3D curves would hold more than `count` curves or `points` points.
fn check_wire_budget(count: usize, points: usize) -> Result<()> {
    if count > crate::link::MAX_LINK_ENTITIES || points > 1_000_000 {
        return Err(SketchError::Invalid("3D curves exceed the stored geometry budget".into()));
    }
    Ok(())
}

impl Sketch {
    fn wire_budget(&self, replacing: Option<&str>, new_count: usize, new_points: usize) -> Result<()> {
        let mut count = new_count;
        let mut points = new_points;
        for w in self.wires.iter().filter(|w| replacing.is_none() || w.link.as_deref() != replacing) {
            count = count.saturating_add(1);
            points = points.saturating_add(w.pts.len()).saturating_add(w.fit.len());
        }
        check_wire_budget(count, points)
    }

    /// Whether a drawn curve (a 3D equation curve, #12662) may take these polyline and fit point
    /// counts in place of wire `index` (`None`: as a new wire).
    pub(crate) fn wire_budget_for(&self, index: Option<usize>, points: usize, fit: usize) -> Result<()> {
        let mut count = 1usize;
        let mut total = points.saturating_add(fit);
        for (i, w) in self.wires.iter().enumerate() {
            if Some(i) == index {
                continue;
            }
            count = count.saturating_add(1);
            total = total.saturating_add(w.pts.len()).saturating_add(w.fit.len());
        }
        check_wire_budget(count, total)
    }

    /// Validate saved arrays without regenerating legacy geometry or changing point values.
    pub fn validate_wires(&self) -> Result<()> {
        if self.wires.len() > crate::link::MAX_LINK_ENTITIES {
            return Err(SketchError::Invalid("too many 3D curves".into()));
        }
        let mut work = 0usize;
        let mut ids = std::collections::BTreeSet::new();
        for w in &self.wires {
            work = work.saturating_add(w.pts.len()).saturating_add(w.fit.len());
            if work > 1_000_000
                || w.id.is_empty()
                || w.id.len() > 512
                || !ids.insert(&w.id)
                || w.link.as_ref().is_some_and(|id| id.is_empty() || id.len() > 512)
                || w.pts.is_empty()
                || w.pts.len() > MAX_WIRE_POINTS
                || w.fit.len() > MAX_FIT_POINTS
                || (w.pts.len() == 1 && w.fit.len() != 1)
                || w.pts.iter().chain(&w.fit).any(|p| !in_range3(*p))
                || w.arc.as_ref().is_some_and(|a| !arc_ok(a, &w.fit))
                || w.control.as_ref().is_some_and(|c| !control_ok(c, w))
            {
                return Err(SketchError::Invalid("3D curve arrays, ids or point bounds are invalid".into()));
            }
        }
        Ok(())
    }

    /// Producer semantics for legacy wires with retained association provenance. Breaking a
    /// link removes this evidence, so detached wires conservatively retain world coordinates.
    pub fn legacy_wire_space(&self, wire: &Wire) -> WireSpace {
        let Some(link) = wire.link.as_deref().and_then(|id| self.link(id)) else { return WireSpace::World };
        WireSpace::from_legacy_link(link)
    }

    /// Whether a curve is reference geometry that planar corner tools cannot modify.
    /// Shared by command validation and the Fillet/Chamfer pick filters.
    pub fn is_spatial_corner_curve(&self, id: &str) -> bool {
        self.wire_index(id).is_some()
    }

    pub fn wire_index(&self, id: &str) -> Option<usize> {
        self.wires.iter().position(|w| w.id == id)
    }

    /// Wires owned by a link, in creation order.
    pub fn link_wires(&self, id: &str) -> Vec<usize> {
        self.wires.iter().enumerate().filter(|(_, w)| w.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Add a link with planar geometry and/or 3D wires.
    pub fn add_link_with_wires(&mut self, kind: LinkKind, source: LinkSource, geom: &[LinkGeom], wires: &[Vec<Vec3>]) -> Result<String> {
        let ws = clean_wires(wires)?;
        self.wire_budget(None, ws.len(), ws.iter().map(Vec::len).sum())?;
        if ws.is_empty() {
            return self.add_link(kind, source, geom);
        }
        let id = if geom.is_empty() {
            let id = self.fresh("j")?;
            self.links.push(Link::new(id.clone(), kind, source));
            id
        } else {
            self.add_link(kind, source, geom)?
        };
        self.set_link_wires(&id, &ws)?;
        Ok(id)
    }

    /// Replace a link's wires (ids are kept in order; extra wires get new ids).
    pub fn set_link_wires(&mut self, id: &str, wires: &[Vec<Vec3>]) -> Result<()> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        let ws = clean_wires(wires)?;
        self.wire_budget(Some(id), ws.len(), ws.iter().map(Vec::len).sum())?;
        let old: Vec<String> = self.link_wires(id).iter().filter_map(|i| self.wires.get(*i).map(|w| w.id.clone())).collect();
        self.wires.retain(|w| w.link.as_deref() != Some(id));
        for (k, pts) in ws.into_iter().enumerate() {
            let wid = match old.get(k) {
                Some(o) => o.clone(),
                None => self.fresh("w")?,
            };
            self.wires.push(Wire {
                id: wid,
                pts,
                space: WireSpace::Component,
                link: Some(id.to_string()),
                fit: Vec::new(),
                arc: None,
                control: None,
                exact: Vec::new(),
            });
        }
        Ok(())
    }

    /// Give a link's wires their exact curves ([`Wire::exact`]), one list per wire in the order
    /// [`Self::set_link_wires`] took them (an empty list: known only as points).
    pub fn set_link_wire_curves(&mut self, id: &str, exact: &[Vec<WireCurve>]) -> Result<()> {
        let wires = self.link_wires(id);
        if wires.len() != exact.len() {
            return Err(SketchError::Invalid(format!("link {id} has {} 3D curves, not {}", wires.len(), exact.len())));
        }
        for (i, curves) in wires.into_iter().zip(exact) {
            if let Some(w) = self.wires.get_mut(i) {
                w.exact = curves.clone();
            }
        }
        Ok(())
    }

    /// Add a curve drawn in a 3D sketch through `fit` (see [`Wire::fit`]).
    pub fn add_drawn_wire(&mut self, fit: &[Vec3]) -> Result<String> {
        self.add_drawn_wire_in(fit, WireSpace::World)
    }

    /// Add a drawn curve without changing the coordinate meaning supplied by its author.
    pub fn add_drawn_wire_in(&mut self, fit: &[Vec3], space: WireSpace) -> Result<String> {
        let pts = drawn_polyline(fit)?;
        self.wire_budget(None, 1, pts.len() + fit.len())?;
        let id = self.fresh(if fit.len() == 1 { "q" } else { "w" })?;
        self.wires.push(Wire { id: id.clone(), pts, space, link: None, fit: fit.to_vec(), arc: None, control: None, exact: Vec::new() });
        Ok(id)
    }

    /// Add an exact circular arc drawn in a 3D sketch: `fit` is its three defining points
    /// (start, through, end) in `space`; the wire keeps the analytic arc and its samples.
    pub fn add_drawn_arc_in(&mut self, fit: &[Vec3], space: WireSpace) -> Result<String> {
        let [start, through, end] = fit else {
            return Err(SketchError::Invalid("a 3D arc needs exactly three points".into()));
        };
        for p in fit {
            if !in_range3(*p) {
                return Err(SketchError::Invalid("3D point out of range".into()));
            }
        }
        let arc = arc_from_3_points(*start, *through, *end)
            .ok_or_else(|| SketchError::Invalid("the three points of a 3D arc must differ and not be collinear".into()))?;
        let pts = arc_polyline(&arc, ARC_TOL);
        self.wire_budget(None, 1, pts.len() + fit.len())?;
        let id = self.fresh("w")?;
        self.wires.push(Wire { id: id.clone(), pts, space, link: None, fit: fit.to_vec(), arc: Some(arc), control: None, exact: Vec::new() });
        Ok(id)
    }

    /// Add a control point spline drawn in a 3D sketch: `ctrl` is its control points in `space`
    /// (three or more); the wire keeps them, its degree and its samples ([`WireControl`]).
    pub fn add_drawn_control_in(&mut self, ctrl: &[Vec3], degree: u8, space: WireSpace) -> Result<String> {
        let pts = drawn_control(ctrl, degree)?;
        self.wire_budget(None, 1, pts.len() + ctrl.len())?;
        let id = self.fresh("w")?;
        self.wires.push(Wire {
            id: id.clone(),
            pts,
            space,
            link: None,
            fit: ctrl.to_vec(),
            arc: None,
            control: Some(WireControl { degree }),
            exact: Vec::new(),
        });
        Ok(id)
    }

    /// Move a fit point of a drawn 3D curve, and every other drawn point at the same place
    /// (curves drawn end to end stay joined). Returns how many points moved.
    pub fn move_drawn_point(&mut self, wire: &str, index: usize, to: Vec3) -> Result<usize> {
        self.move_drawn_points(&[(wire.to_string(), index, to)])
    }

    /// [`Sketch::move_drawn_point`] for several points at once: every place is read before any
    /// point moves, and a place picked more than once moves once, to its first target.
    pub fn move_drawn_points(&mut self, moves: &[(String, usize, Vec3)]) -> Result<usize> {
        let mut targets: Vec<(WireSpace, Vec3, Vec3)> = Vec::new();
        for (wire, index, to) in moves {
            if !in_range3(*to) {
                return Err(SketchError::Invalid("3D point out of range".into()));
            }
            let w = self.wire_index(wire).ok_or_else(|| SketchError::Unknown(wire.clone()))?;
            let space = self
                .wires
                .get(w)
                .filter(|w| w.link.is_none())
                .map(|w| w.space)
                .ok_or_else(|| SketchError::Invalid("linked 3D geometry cannot be moved".into()))?;
            let from =
                self.wires.get(w).and_then(|w| w.fit.get(*index)).copied().ok_or_else(|| SketchError::Unknown(format!("{wire} point {index}")))?;
            if !targets.iter().any(|(sp, f, _)| *sp == space && f.dist(from) < 1e-9) {
                targets.push((space, from, *to));
            }
        }
        let mut n = 0;
        let mut changes = Vec::new();
        for (index, w) in self.wires.iter().enumerate().filter(|(_, w)| !w.fit.is_empty() && w.link.is_none()) {
            let mut fit = w.fit.clone();
            let mut changed = false;
            for p in fit.iter_mut() {
                if let Some((_, _, to)) = targets.iter().find(|(sp, f, _)| *sp == w.space && p.dist(*f) < 1e-9) {
                    *p = *to;
                    changed = true;
                    n += 1;
                }
            }
            if changed {
                // An exact arc keeps its analytic form: re-derive it from its three defining
                // points; a spline keeps its polyline, through its fit points or over its
                // control points. All before any change lands.
                let (arc, pts) = match w.arc {
                    Some(_) => {
                        let [start, through, end] = fit.as_slice() else {
                            return Err(SketchError::Invalid("a 3D arc needs exactly three points".into()));
                        };
                        let arc = arc_from_3_points(*start, *through, *end)
                            .ok_or_else(|| SketchError::Invalid("a 3D arc's points must differ and not be collinear".into()))?;
                        (Some(arc), arc_polyline(&arc, ARC_TOL))
                    }
                    None => (None, w.samples_of(&fit)?),
                };
                changes.push((index, fit, arc, pts));
            }
        }
        for (index, fit, arc, pts) in changes {
            if let Some(w) = self.wires.get_mut(index) {
                w.fit = fit;
                w.arc = arc;
                w.pts = pts;
            }
        }
        Ok(n)
    }

    /// Remove wires by id.
    pub fn remove_wires(&mut self, ids: &[String]) {
        self.wires.retain(|w| !ids.contains(&w.id));
        self.prune_wire_constraints();
        self.prune_equations(&ids.iter().cloned().collect());
        self.prune_links();
    }
}

#[cfg(test)]
mod tests {
    use solidcore_record::Vec2;

    use super::*;

    #[test]
    fn wire_space_defaults_and_preserves_detached_provenance() {
        let mut sk = Sketch::new();
        let id = sk.add_drawn_wire(&[Vec3::new(1.0, 2.0, 3.0)]).unwrap();
        assert_eq!(sk.wires[0].space, WireSpace::World);
        let mut raw = serde_json::to_value(&sk.wires[0]).unwrap();
        raw.as_object_mut().unwrap().remove("space");
        assert_eq!(serde_json::from_value::<Wire>(raw).unwrap().space, WireSpace::World);
        let link = sk
            .add_link_with_wires(
                LinkKind::Include,
                LinkSource::Edge { body: "Missing".into(), at: Vec3::ZERO },
                &[],
                &[vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 2.0, 4.0)]],
            )
            .unwrap();
        sk.set_link_lost(&link, true);
        let wire = sk.wires.iter().find(|w| w.link.as_deref() == Some(&link)).unwrap();
        assert_eq!(sk.legacy_wire_space(wire), WireSpace::Component);
        let wid = wire.id.clone();
        sk.break_link(&link).unwrap();
        let wire = sk.wires.iter().find(|w| w.id == wid).unwrap();
        assert_eq!(wire.space, WireSpace::Component);
        assert_eq!(sk.legacy_wire_space(wire), WireSpace::World);
        assert_eq!(sk.wires.iter().find(|w| w.id == id).unwrap().space, WireSpace::World);
    }

    #[test]
    fn wire_space_invalid_producers_and_move_are_atomic() {
        let mut sk = Sketch::new();
        let wire = sk.add_drawn_wire_in(&[Vec3::ZERO, Vec3::Z], WireSpace::Component).unwrap();
        sk.add_drawn_wire(&[Vec3::ZERO]).unwrap();
        assert_eq!(sk.move_drawn_point(&wire, 0, Vec3::X).unwrap(), 1);
        assert_eq!(sk.wires[1].fit[0], Vec3::ZERO);
        let before = sk.clone();
        assert!(sk.move_drawn_point(&wire, 0, Vec3::Z).is_err());
        assert_eq!(sk, before);
        assert!(sk.add_link_with_wires(LinkKind::Include, LinkSource::Origin, &[], &[vec![Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)]]).is_err());
        assert_eq!(sk, before);
        assert!(sk.add_link_with_wires(LinkKind::Include, LinkSource::Origin, &[], &[vec![Vec3::ZERO; MAX_WIRE_POINTS + 1]]).is_err());
        assert_eq!(sk, before);
        sk.validate_wires().unwrap();
    }

    /// Through points around a circle that end where they start, the spline turns as smoothly at
    /// the closing point as everywhere else; an open one keeps its mirrored end tangents.
    #[test]
    fn a_closed_spline_is_smooth_where_it_closes() {
        let ring: Vec<Vec3> = (0..=12).map(|k| f64::from(k) * std::f64::consts::TAU / 12.0).map(|a| Vec3::new(a.cos(), a.sin(), 0.0)).collect();
        let pts = fit_polyline(&ring);
        let dir = |a: Vec3, b: Vec3| (b - a).normalized().unwrap();
        let n = pts.len();
        let (start, end) = (dir(pts[0], pts[1]), dir(pts[n - 2], pts[n - 1]));
        let mid = |i: usize| dir(pts[i - 1], pts[i]).dot(dir(pts[i], pts[i + 1]));
        assert!(
            start.dot(end) >= mid(solidcraft_geom::catmull_rom::SPAN_SAMPLES) - 1e-9,
            "turn at the close {}, at a fit point {}",
            start.dot(end),
            mid(solidcraft_geom::catmull_rom::SPAN_SAMPLES)
        );
        let open = fit_polyline(&ring[..12]);
        assert_eq!(open.first(), ring.first());
        assert_eq!(open.last(), ring.get(11));
    }

    #[test]
    fn an_arc_from_three_points_is_exact() {
        // A quarter circle of radius 10 in the XY plane: 0° through 45° to 90°.
        let arc = arc_from_3_points(
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(5.0 * std::f64::consts::SQRT_2, 5.0 * std::f64::consts::SQRT_2, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
        )
        .unwrap();
        assert!((arc.radius - 10.0).abs() < 1e-9, "radius {}", arc.radius);
        assert!(arc.center.dist(Vec3::ZERO) < 1e-9, "centre {:?}", arc.center);
        assert!((arc_sweep(&arc) - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "sweep {}", arc_sweep(&arc));
        // A tilted plane: the same arc lifted onto a 45° plane keeps its radius and sweep.
        let n = std::f64::consts::FRAC_PI_4;
        let up = Vec3::new(0.0, n.sin(), n.cos());
        let right = Vec3::new(1.0, 0.0, 0.0);
        let at = |a: f64| up * (10.0 * a.sin()) + right * (10.0 * a.cos());
        let tilted = arc_from_3_points(at(0.0), at(std::f64::consts::FRAC_PI_4), at(std::f64::consts::FRAC_PI_2)).unwrap();
        assert!((tilted.radius - 10.0).abs() < 1e-9);
        assert!((arc_sweep(&tilted) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(tilted.normal.dot(up.cross(right).normalized().unwrap()).abs() > 1.0 - 1e-9);
    }

    #[test]
    fn an_arc_refuses_degenerate_and_hostile_points() {
        // Collinear, coincident and out-of-range defining points never build an arc.
        assert!(arc_from_3_points(Vec3::ZERO, Vec3::X, Vec3::X * 2.0).is_none());
        assert!(arc_from_3_points(Vec3::ZERO, Vec3::ZERO, Vec3::Y).is_none());
        assert!(arc_from_3_points(Vec3::ZERO, Vec3::new(1e-12, 0.0, 0.0), Vec3::Y).is_none());
        let mut sk = Sketch::new();
        assert!(sk.add_drawn_arc_in(&[Vec3::ZERO, Vec3::X, Vec3::X * 2.0], WireSpace::World).is_err());
        assert!(sk.add_drawn_arc_in(&[Vec3::ZERO, Vec3::X], WireSpace::World).is_err());
        assert!(sk.add_drawn_arc_in(&[Vec3::ZERO, Vec3::new(1e100, 0.0, 0.0), Vec3::Y], WireSpace::World).is_err());
        assert!(sk.add_drawn_arc_in(&[Vec3::ZERO, Vec3::new(f64::NAN, 1.0, 0.0), Vec3::Y], WireSpace::World).is_err());
        assert!(sk.wires.is_empty());
    }

    #[test]
    fn an_arc_wire_stores_fit_points_samples_and_survives_a_round_trip() {
        let mut sk = Sketch::new();
        let id =
            sk.add_drawn_arc_in(&[Vec3::new(10.0, 0.0, 5.0), Vec3::new(0.0, 10.0, 5.0), Vec3::new(-10.0, 0.0, 5.0)], WireSpace::Component).unwrap();
        let w = sk.wires.iter().find(|w| w.id == id).unwrap();
        assert_eq!(w.fit.len(), 3);
        assert!(w.arc.is_some());
        assert!(w.pts.len() >= 3);
        assert_eq!(w.pts.first(), w.fit.first());
        assert_eq!(w.pts.last(), w.fit.get(2));
        // Every sample sits on the circle.
        let arc = w.arc.unwrap();
        assert!(w.pts.iter().all(|p| (arc.center.dist(*p) - arc.radius).abs() < 1e-6));
        sk.validate_wires().unwrap();
        let raw = serde_json::to_value(w).unwrap();
        assert_eq!(serde_json::from_value::<Wire>(raw).unwrap(), *w);
    }

    #[test]
    fn moving_an_arc_point_rebuilds_it_and_refuses_collinear() {
        let mut sk = Sketch::new();
        let id = sk.add_drawn_arc_in(&[Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(-10.0, 0.0, 0.0)], WireSpace::World).unwrap();
        // Pull the through point onto the chord: collinear, refused, and nothing changed.
        let before = sk.clone();
        assert!(sk.move_drawn_point(&id, 1, Vec3::ZERO).is_err());
        assert_eq!(sk, before);
        // Pull it off again: the arc follows and stays exact.
        sk.move_drawn_point(&id, 1, Vec3::new(0.0, 5.0, 0.0)).unwrap();
        let w = sk.wires.iter().find(|w| w.id == id).unwrap();
        assert_eq!(w.fit[1], Vec3::new(0.0, 5.0, 0.0));
        let arc = w.arc.unwrap();
        assert!(w.pts.iter().all(|p| (arc.center.dist(*p) - arc.radius).abs() < 1e-6));
        sk.validate_wires().unwrap();
    }

    /// Five control points off any one plane, for the control point spline tests.
    fn five() -> [Vec3; 5] {
        [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 20.0, 5.0), Vec3::new(30.0, 25.0, -10.0), Vec3::new(50.0, 0.0, 15.0), Vec3::new(60.0, -20.0, 40.0)]
    }

    #[test]
    fn a_control_spline_touches_only_its_end_control_points() {
        // Four control points of a cubic are one Bézier: its middle is (P0 + 3 P1 + 3 P2 + P3) / 8.
        let c = five();
        let pts = control_polyline(&c[..4], 3);
        assert_eq!(pts.len(), CONTROL_SPAN_SAMPLES + 1);
        assert_eq!((pts.first(), pts.last()), (c.first(), c.get(3)));
        let mid = (c[0] + c[1] * 3.0 + c[2] * 3.0 + c[3]) * 0.125;
        assert!(pts[CONTROL_SPAN_SAMPLES / 2].dist(mid) < 1e-12, "{:?} is not {mid:?}", pts[CONTROL_SPAN_SAMPLES / 2]);
        // The inner control points pull the curve; it does not pass through them.
        assert!(pts.iter().all(|p| p.dist(c[1]) > 1.0 && p.dist(c[2]) > 1.0));
        // Three control points of a cubic are a quadratic Bézier: its middle is (P0 + 2 P1 + P2) / 4.
        let quadratic = control_polyline(&c[..3], 3);
        assert_eq!(quadratic.len(), CONTROL_SPAN_SAMPLES + 1);
        assert!(quadratic[CONTROL_SPAN_SAMPLES / 2].dist((c[0] + c[1] * 2.0 + c[2]) * 0.25) < 1e-12);
    }

    #[test]
    fn a_control_spline_of_two_spans_joins_them_where_the_knot_says() {
        // Five control points of a cubic: knots 0 0 0 0 ½ 1 1 1 1. Inserting ½ twice more (or
        // the blossom f(½, ½, ½)) puts the joint of the two spans at (P1 + 2 P2 + P3) / 4.
        let c = five();
        let pts = control_polyline(&c, 3);
        assert_eq!(pts.len(), 2 * CONTROL_SPAN_SAMPLES + 1);
        assert_eq!((pts.first(), pts.last()), (c.first(), c.last()));
        let joint = (c[1] + c[2] * 2.0 + c[3]) * 0.25;
        assert!(pts[CONTROL_SPAN_SAMPLES].dist(joint) < 1e-12, "{:?} is not {joint:?}", pts[CONTROL_SPAN_SAMPLES]);
        let Some(WireCurve { curve: solidcore_record::brep::Curve::Spline { knots, points }, interval }) = control_curve(&c, 3) else {
            panic!("a control spline is a record spline")
        };
        assert_eq!(knots, vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(points, c.map(|p| [p.x, p.y, p.z]).to_vec());
        assert_eq!(interval, [0.0, 1.0]);
    }

    #[test]
    fn a_control_spline_in_a_plane_is_the_planar_control_spline() {
        // The same control points on the sketch plane, by the planar spline's own evaluation.
        let flat: Vec<Vec2> = (0..9).map(|i| Vec2::new(10.0 * f64::from(i), [0.0, 12.0, -7.0, 20.0, 3.0][i as usize % 5])).collect();
        let lifted: Vec<Vec3> = flat.iter().map(|p| Vec3::new(p.x, p.y, 4.0)).collect();
        for degree in [1u8, 2, 3, 5] {
            let pts = control_polyline(&lifted, degree);
            let planar = crate::curves::spline_polyline(&flat, true, degree);
            assert_eq!(pts.len(), planar.len(), "degree {degree}");
            for (p, q) in pts.iter().zip(&planar) {
                assert!(p.dist(Vec3::new(q.x, q.y, 4.0)) < 1e-11, "degree {degree}: {p:?} is not {q:?}");
            }
        }
    }

    #[test]
    fn a_control_wire_keeps_its_control_points_through_a_move_and_a_round_trip() {
        let mut sk = Sketch::new();
        let c = five();
        let id = sk.add_drawn_control_in(&c, 3, WireSpace::Component).unwrap();
        let w = sk.wires.iter().find(|w| w.id == id).unwrap().clone();
        assert_eq!(w.fit, c.to_vec());
        assert_eq!(w.control, Some(WireControl { degree: 3 }));
        assert_eq!(w.pts, control_polyline(&c, 3));
        assert!(w.arc.is_none() && w.control_curve().is_some());
        sk.validate_wires().unwrap();
        let raw = serde_json::to_value(&w).unwrap();
        assert_eq!(raw["control"], serde_json::json!({"degree": 3}));
        assert_eq!(serde_json::from_value::<Wire>(raw).unwrap(), w);
        // A fit-point curve saves no such field, and a design from before it reads as one.
        let fit = sk.add_drawn_wire_in(&c, WireSpace::Component).unwrap();
        let mut raw = serde_json::to_value(sk.wires.iter().find(|w| w.id == fit).unwrap()).unwrap();
        assert!(raw.get("control").is_none());
        raw.as_object_mut().unwrap().remove("arc");
        assert_eq!(serde_json::from_value::<Wire>(raw).unwrap().control, None);
        // Dragging a control point: the curve is still the spline over the control points, and
        // the fit-point curve that shares the place moves with it (curves drawn end to end).
        let to = Vec3::new(30.0, 40.0, 0.0);
        assert_eq!(sk.move_drawn_point(&id, 2, to).unwrap(), 2);
        let moved = sk.wires.iter().find(|w| w.id == id).unwrap();
        let mut want = c;
        want[2] = to;
        assert_eq!(moved.fit, want.to_vec());
        assert_eq!(moved.pts, control_polyline(&want, 3));
        assert_ne!(moved.pts, fit_polyline(&want));
        assert_eq!(moved.control, Some(WireControl { degree: 3 }));
        sk.validate_wires().unwrap();
    }

    #[test]
    fn a_control_spline_refuses_hostile_points_and_leaves_the_sketch_alone() {
        let mut sk = Sketch::new();
        let c = five();
        let before = sk.clone();
        // Two control points are a line; a degree of 0 or above the exact pieces' is no spline.
        assert!(sk.add_drawn_control_in(&c[..2], 3, WireSpace::World).is_err());
        assert!(sk.add_drawn_control_in(&c, 0, WireSpace::World).is_err());
        assert!(sk.add_drawn_control_in(&c, 10, WireSpace::World).is_err());
        assert!(sk.add_drawn_control_in(&[Vec3::X; 3], 3, WireSpace::World).is_err());
        assert!(sk.add_drawn_control_in(&[c[0], Vec3::new(f64::NAN, 0.0, 0.0), c[2]], 3, WireSpace::World).is_err());
        assert!(sk.add_drawn_control_in(&[c[0], Vec3::new(1e100, 0.0, 0.0), c[2]], 3, WireSpace::World).is_err());
        let many: Vec<Vec3> = (0..=MAX_FIT_POINTS).map(|i| Vec3::new(i as f64, 0.0, 0.0)).collect();
        assert!(sk.add_drawn_control_in(&many, 3, WireSpace::World).is_err());
        assert_eq!(sk, before);
        assert!(control_polyline(&[], 3).is_empty() && control_curve(&c[..1], 3).is_none() && control_curve(&c, 0).is_none());
        // A control point dragged onto its neighbour is refused, and nothing changed.
        let id = sk.add_drawn_control_in(&c, 3, WireSpace::World).unwrap();
        let before = sk.clone();
        assert!(sk.move_drawn_point(&id, 1, c[2]).is_err());
        assert_eq!(sk, before);
    }

    #[test]
    fn a_saved_control_spline_that_cannot_be_one_is_refused() {
        let mut sk = Sketch::new();
        sk.add_drawn_control_in(&five(), 3, WireSpace::World).unwrap();
        sk.validate_wires().unwrap();
        let broken = |change: &dyn Fn(&mut Wire)| {
            let mut bad = sk.clone();
            change(&mut bad.wires[0]);
            bad.validate_wires().is_err()
        };
        assert!(broken(&|w| w.control = Some(WireControl { degree: 0 })));
        assert!(broken(&|w| w.control = Some(WireControl { degree: 10 })));
        assert!(broken(&|w| w.fit.truncate(2)));
        assert!(broken(&|w| w.link = Some("j1".into())));
        assert!(broken(&|w| w.arc = arc_from_3_points(Vec3::X, Vec3::Y, Vec3::Z)));
        assert!(broken(&|w| w.exact = w.control_curve().into_iter().collect()));
    }

    #[test]
    fn a_saved_arc_that_lies_about_its_circle_is_refused() {
        let mut sk = Sketch::new();
        sk.add_drawn_arc_in(&[Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(-10.0, 0.0, 0.0)], WireSpace::World).unwrap();
        sk.wires[0].arc.as_mut().unwrap().radius = 1.0; // claims a circle its ends are not on
        assert!(sk.validate_wires().is_err());
    }
}
