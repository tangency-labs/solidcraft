//! Filling a closed loop of a body's edges with one face, bounded by the body's own edges,
//! whole: the loop's plane when its edges certainly lie in one, otherwise a B-spline height field
//! over the loop's best-fit plane (`solidcraft_geom::fill::fill_loop`). The face is assembled on
//! the body's own edge records (`sew::assemble`) and built by SMLib. The height field's
//! parameters are plane coordinates, so each edge's pcurve is its exact projection onto the
//! plane.
//!
//! The general fill (`solidcraft_geom::fill_general`), which also meets the faces beside the loop
//! with G1 or G2 and passes through given points, is not ported yet: its surface's parameters are
//! developed from the loop, so the loop's edges have no exact pcurve on it, and the frames across
//! each edge come from `bridge::frames_across`.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self as b, Curve2, eval::CurveEval};
use solidcraft_geom::bspline::{HPoint, SplineError};
use solidcraft_geom::fill_general::MAX_FILL_POINTS;

use crate::body::Body;
use crate::bridge::Continuity;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceProv, History};
use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
use crate::{KernelError, Result};

/// How far (degrees) a G1 or G2 patch's normal may turn from its neighbour's along the loop.
pub const PATCH_ANGLE_TOL_DEG: f64 = 0.05;

/// A spline error as the patch reports it.
fn spline_error(e: SplineError) -> KernelError {
    match e {
        SplineError::Unsupported(m) => KernelError::Invalid(format!("not supported yet: {m}")),
        other => KernelError::Invalid(format!("cannot fill the loop: {other}")),
    }
}

/// What a patch on a loop of edges must do besides filling it.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchSpec {
    /// How every edge's neighbour is met (G0, G1 or G2).
    pub continuity: Continuity,
    /// Edges met otherwise: a point on the edge and its continuity.
    pub edges: Vec<(Vec3, Continuity)>,
    /// Points the patch passes through.
    pub points: Vec<Vec3>,
    /// How full the patch's middle is, `crown` times the loop's size (−0.9…3): the bump of
    /// [`patch_edges_crowned`], laid along the general fill's own normal there (to the side its
    /// G1/G2 neighbours' normals face) on control points clear of the loop, so the loop and the
    /// conditions across it do not move. A general fill needs a G1 or G2 edge to crown.
    pub crown: f64,
}

impl Default for PatchSpec {
    fn default() -> PatchSpec {
        PatchSpec { continuity: Continuity::G0, edges: Vec::new(), points: Vec::new(), crown: 0.0 }
    }
}

impl PatchSpec {
    /// Whether this is the plain patch of [`patch_edges_crowned`]: G0 everywhere, no points.
    pub fn is_plain(&self) -> bool {
        self.continuity == Continuity::G0 && self.edges.iter().all(|(_, g)| *g == Continuity::G0) && self.points.is_empty()
    }
}

/// What a general fill achieved (all zero for a planar or height-field patch, which meet their
/// edges within [`face_tolerance`] and ask nothing else).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PatchReport {
    /// Whether the general fill made the face.
    pub general: bool,
    /// The farthest loop station from the surface.
    pub gap: f64,
    /// The largest angle (degrees) between the patch's normal and a G1/G2 neighbour's, away from
    /// corners where the neighbours themselves disagree, and anywhere.
    pub normal_deviation_deg: f64,
    pub normal_deviation_all_deg: f64,
    /// The largest difference (1/mm) between the patch's normal curvature across a G2 edge and
    /// the neighbour's.
    pub curvature_mismatch: f64,
    /// The farthest point from the surface.
    pub point_gap: f64,
}

/// [`crate::patch_edges`] meeting the faces beside the loop with the continuity `spec` asks for
/// and passing through its points; a plain spec is exactly [`patch_edges_crowned`]. Its history
/// is over `[body]`: the face is new, its edges the body's own.
pub fn patch_edges_with(body: &Body, at: &[Vec3], spec: &PatchSpec) -> Result<Body> {
    patch_edges_report(body, at, spec).map(|(b, _)| b)
}

/// [`patch_edges_with`] and what the fill achieved.
pub fn patch_edges_report(body: &Body, at: &[Vec3], spec: &PatchSpec) -> Result<(Body, PatchReport)> {
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    // A range test also throws out NaN.
    if !(solidcraft_geom::fill::MIN_CROWN..=solidcraft_geom::fill::MAX_CROWN).contains(&spec.crown) {
        return Err(KernelError::Invalid("a patch's crown must be between -0.9 and 3".into()));
    }
    let g3 = |g: Continuity| g == Continuity::G3;
    if g3(spec.continuity) || spec.edges.iter().any(|(_, g)| g3(*g)) {
        return Err(KernelError::Invalid("not supported yet: a patch meets its neighbours with g0, g1 or g2".into()));
    }
    if spec.edges.len() > 10_000 || spec.edges.iter().any(|(p, _)| !p.is_finite()) {
        return Err(KernelError::Invalid("pick at most 10000 edges, each by a finite point, for their continuity".into()));
    }
    if spec.points.len() > MAX_FILL_POINTS || spec.points.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid(format!("a patch passes through at most {MAX_FILL_POINTS} finite points")));
    }
    if spec.is_plain() {
        return patch_edges_crowned(body, at, spec.crown).map(|b| (b, PatchReport::default()));
    }
    body.require_brep("patch")?;
    Err(general_fill_unported())
}

/// The general fill's refusal until it is ported (module docs).
fn general_fill_unported() -> KernelError {
    KernelError::Invalid(
        "not supported yet: a patch meeting its neighbours with g1 or g2, through points, or on a loop that bounds no height field".into(),
    )
}

/// [`crate::patch_edges`] with a crown on the fitted face: positive makes the patch's middle
/// fuller, negative flatter, by `crown` times the loop's size (its projected box's diagonal), the
/// bump vanishing along the loop, so the face still meets the body's own edges. `crown` 0 is
/// exactly [`crate::patch_edges`], and a loop that is certainly planar makes the same exact planar
/// face whatever the crown. Its history is over `[body]`: the face is new, its edges the body's
/// own.
pub fn patch_edges_crowned(body: &Body, at: &[Vec3], crown: f64) -> Result<Body> {
    body.require_brep("patch")?;
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    // A range test also throws out NaN.
    if !(solidcraft_geom::fill::MIN_CROWN..=solidcraft_geom::fill::MAX_CROWN).contains(&crown) {
        return Err(KernelError::Invalid("a patch's crown must be between -0.9 and 3".into()));
    }
    patch_loop(body, at, crown)
}

/// The patch on the closed loop of `body`'s edges picked by `at`: the loop's plane when it is
/// certainly planar (within [`planarity_tolerance`]), else the height field crowned by `crown`.
/// What `crate::patch_edges` makes with `crown` 0.
pub(crate) fn patch_loop(body: &Body, at: &[Vec3], crown: f64) -> Result<Body> {
    let record = crate::native::brep_record(body)?;
    let reach = body.size() * 0.05 + 1e-6;
    let chain = picked_loop(&record, at, reach)?;
    let mut sides = Vec::with_capacity(chain.len());
    for &(e, forward) in &chain {
        sides.push(Side::new(&record, e, forward)?);
    }
    let (surface, ccw, pcurve): (b::Surface, bool, Option<Projection>) = match planar(&record, &chain, &mut sides, planarity_tolerance(body))? {
        Some(plane) => (plane, true, None),
        None => {
            let tol = face_tolerance(body);
            let mut point = |k: usize, t: f64| sides.get_mut(k).and_then(|s| s.point(t));
            let fill = match solidcraft_geom::fill::fill_loop(chain.len(), &mut point, tol, crown) {
                Ok(fill) => fill,
                Err(SplineError::Unsupported(_)) if crown == 0.0 => return Err(general_fill_unported()),
                Err(e) => return Err(spline_error(e)),
            };
            let projection = Projection { origin: fill.origin, axes: fill.axes };
            (surface_record(&fill.surface)?, fill.counter_clockwise, Some(projection))
        }
    };
    patch_body(body, &record, &chain, surface, ccw, pcurve.as_ref(), face_tolerance(body))
}

/// The closed loop of `record`'s edges picked by points on them (the edge nearest each, within
/// `reach`), each with whether the loop runs along it: it starts with the last edge picked, run
/// its own way. Edges that branch, or do not close one loop, are refused.
fn picked_loop(record: &b::Brep, at: &[Vec3], reach: f64) -> Result<Vec<(usize, bool)>> {
    let mut picked: Vec<usize> = Vec::new();
    for p in at {
        if !p.is_finite() {
            return Err(KernelError::Invalid(format!("no edge at {p:?}")));
        }
        let e = nearest_edge(record, *p, reach).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !picked.contains(&e) {
            picked.push(e);
        }
    }
    let ends = |e: usize| record.edges.get(e).map(|x| x.vertices);
    let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for &e in &picked {
        for v in ends(e).into_iter().flatten() {
            *uses.entry(v).or_default() += 1;
        }
    }
    if uses.values().any(|n| *n > 2) {
        return Err(KernelError::Invalid("the edges branch: pick one chain".into()));
    }
    let open = || KernelError::Invalid("the edges do not form a closed loop".into());
    if uses.values().any(|n| *n != 2) {
        return Err(open());
    }
    let first = picked.pop().ok_or_else(open)?;
    let [start, mut at_v] = ends(first).ok_or_else(open)?;
    let mut chain = vec![(first, true)];
    while at_v != start {
        let k = picked.iter().position(|e| ends(*e).is_some_and(|[a, z]| a == at_v || z == at_v)).ok_or_else(open)?;
        let e = picked.remove(k);
        let [a, z] = ends(e).ok_or_else(open)?;
        let forward = a == at_v;
        at_v = if forward { z } else { a };
        chain.push((e, forward));
    }
    if !picked.is_empty() {
        return Err(open());
    }
    Ok(chain)
}

/// The record edge whose curve comes nearest `p` (by samples along each), within `reach`.
fn nearest_edge(record: &b::Brep, p: Vec3, reach: f64) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (i, e) in record.edges.iter().enumerate() {
        let Ok(c) = CurveEval::new(&e.curve) else { continue };
        let [t0, t1] = e.interval;
        for k in 0..=32 {
            let Some(q) = c.point(t0 + (t1 - t0) * k as f64 / 32.0) else { continue };
            let d = q.dist(p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
    }
    best.filter(|(d, _)| *d <= reach).map(|(_, e)| e)
}

/// One edge of the loop as the fill samples it: `t` from 0 at the loop's start of the edge to 1 at
/// its end, the start being the edge's own vertex (an imported edge's curve may end a little off
/// it).
struct Side {
    curve: CurveEval,
    interval: [f64; 2],
    forward: bool,
    start: Vec3,
}

impl Side {
    fn new(record: &b::Brep, e: usize, forward: bool) -> Result<Side> {
        let edge = record.edges.get(e).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
        let curve = CurveEval::new(&edge.curve).map_err(|err| KernelError::Invalid(format!("edge {e} cannot be sampled: {err}")))?;
        let v = edge.vertices[if forward { 0 } else { 1 }];
        let start = record.vertices.get(v as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]));
        let start = start.ok_or_else(|| KernelError::Failed("patch: vertex reference".into()))?;
        Ok(Side { curve, interval: edge.interval, forward, start })
    }

    fn point(&mut self, t: f64) -> Option<Vec3> {
        if t <= 0.0 {
            return Some(self.start);
        }
        let [a, z] = self.interval;
        let s = t.min(1.0);
        self.curve.point(if self.forward { a + (z - a) * s } else { z - (z - a) * s })
    }
}

/// The plane of the loop when its edges certainly lie in one within `tol`, `None` otherwise. The
/// plane is fitted to points along the edges (through their mean, its normal the loop's area
/// vector, so the loop runs counter-clockwise about it); each edge is then certified
/// conservatively: a line by its ends, a polyline or spline by its control points (the curve lies
/// in their convex hull, weights positive), a circle or ellipse by its centre and how far its
/// axes tilt out of the plane, a traced intersection only when one of its two surfaces is a plane
/// that the loop's points lie on. Samples alone are never trusted: a spline can bulge out of the
/// plane between them.
fn planar(record: &b::Brep, chain: &[(usize, bool)], sides: &mut [Side], tol: f64) -> Result<Option<b::Surface>> {
    const SAMPLES: usize = 16;
    let mut pts = Vec::with_capacity(sides.len() * SAMPLES);
    for side in sides.iter_mut() {
        for k in 0..SAMPLES {
            let Some(p) = side.point(k as f64 / SAMPLES as f64) else { return Ok(None) };
            pts.push(p);
        }
    }
    let mean = pts.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / pts.len().max(1) as f64);
    let area = pts.iter().zip(pts.iter().cycle().skip(1)).fold(Vec3::ZERO, |n, (p, q)| n + (*p - mean).cross(*q - mean));
    let Some(n) = area.normalized() else { return Ok(None) };
    let off = |p: Vec3| (p - mean).dot(n).abs();
    let on = |p: [f64; 3]| off(Vec3::new(p[0], p[1], p[2])) <= tol;
    if !pts.iter().all(|p| off(*p) <= tol) {
        return Ok(None);
    }
    let holds = |s: &b::Surface| -> bool {
        let (o, normal) = match s {
            b::Surface::Plane { origin, u_axis, v_axis } => {
                let (u, v) = (Vec3::new(u_axis[0], u_axis[1], u_axis[2]), Vec3::new(v_axis[0], v_axis[1], v_axis[2]));
                (Vec3::new(origin[0], origin[1], origin[2]), u.cross(v).normalized())
            }
            _ => return false,
        };
        let Some(q) = normal else { return false };
        pts.iter().all(|p| (*p - o).dot(q).abs() <= tol && (*p - n * (*p - mean).dot(n) - o).dot(q).abs() <= tol)
    };
    for &(e, _) in chain {
        let Some(edge) = record.edges.get(e) else { return Ok(None) };
        let certified = match &edge.curve {
            b::Curve::Line { start, end } => on(*start) && on(*end),
            b::Curve::Polyline { points } | b::Curve::Spline { points, .. } => points.iter().all(|p| on(*p)),
            b::Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().all(|h| h[3] > 0.0 && on([h[0] / h[3], h[1] / h[3], h[2] / h[3]])),
            b::Curve::Circle { frame, radius } => conic_on(frame, [*radius; 2], &on, n, tol),
            b::Curve::Ellipse { frame, radii } => conic_on(frame, *radii, &on, n, tol),
            b::Curve::SurfaceIntersection { surfaces, .. } => surfaces.iter().any(|s| holds(s)),
        };
        if !certified {
            return Ok(None);
        }
    }
    let t = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let Some(u) = (t - n * t.dot(n)).normalized() else { return Ok(None) };
    let arr = |p: Vec3| [p.x, p.y, p.z];
    Ok(Some(b::Surface::Plane { origin: arr(mean), u_axis: arr(u), v_axis: arr(n.cross(u)) }))
}

/// A conic lies within `tol` of the plane (normal `n`) when its centre does and its axes, scaled by
/// its radii, tilt out of the plane by no more than that together.
fn conic_on(frame: &b::Frame, radii: [f64; 2], on: &dyn Fn([f64; 3]) -> bool, n: Vec3, tol: f64) -> bool {
    let axis = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]).dot(n).abs();
    on(frame.origin) && radii[0] * axis(frame.x_axis) + radii[1] * axis(frame.y_axis) <= tol
}

/// The plane a height field stands on: its parameters are coordinates along `axes` from `origin`.
struct Projection {
    origin: Vec3,
    axes: [Vec3; 2],
}

impl Projection {
    /// The edge's exact pcurve on the height field: its curve projected onto the plane, which is
    /// affine, so a line or spline stays one with its knots and weights. `None` for a conic or a
    /// traced intersection, whose angle or trace parameter no B-spline in (u, v) reproduces.
    fn pcurve(&self, curve: &b::Curve, [a, z]: [f64; 2]) -> Option<Curve2> {
        let uv = |p: Vec3| [(p - self.origin).dot(self.axes[0]), (p - self.origin).dot(self.axes[1])];
        let v = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
        let (degree, knots, ctrl): (usize, Vec<f64>, Vec<HPoint>) = match curve {
            b::Curve::Line { start, end } => {
                let (s, e) = (v(start), v(end));
                let (p, q) = (s + (e - s) * a, s + (e - s) * z);
                return Some(Curve2::Spline { knots: vec![a, a, z, z], points: vec![uv(p), uv(q)] });
            }
            b::Curve::Polyline { points } => {
                let mut knots = vec![0.0];
                knots.extend((0..points.len()).map(|i| i as f64));
                knots.push(points.len().saturating_sub(1) as f64);
                (1, knots, points.iter().map(|p| HPoint::weighted(v(p), 1.0)).collect())
            }
            b::Curve::Spline { knots, points } => {
                (knots.len().checked_sub(points.len() + 1)?, knots.clone(), points.iter().map(|p| HPoint::weighted(v(p), 1.0)).collect())
            }
            b::Curve::RationalSpline { knots, homogeneous } => (
                knots.len().checked_sub(homogeneous.len() + 1)?,
                knots.clone(),
                homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect(),
            ),
            b::Curve::Circle { .. } | b::Curve::Ellipse { .. } | b::Curve::SurfaceIntersection { .. } => return None,
        };
        // The piece on the edge's own interval (the same points at the same parameters).
        let domain = [*knots.get(degree)?, *knots.get(knots.len() - degree - 1)?];
        let (knots, ctrl) = if domain == [a, z] { (knots, ctrl) } else { solidcraft_geom::bspline::segment(degree, &knots, &ctrl, a, z).ok()? };
        let homogeneous = ctrl
            .iter()
            .map(|h| {
                let p = uv(Vec3::new(h.x / h.w, h.y / h.w, h.z / h.w));
                [p[0] * h.w, p[1] * h.w, h.w]
            })
            .collect();
        Some(Curve2::RationalSpline { knots, homogeneous })
    }
}

/// A geom NURBS surface as a record: a plain spline where every weight is one.
fn surface_record(s: &solidcraft_geom::bspline::NurbsSurface) -> Result<b::Surface> {
    let [ku, kv] = s.knots().clone();
    let rows = s.net().into_u_rows();
    if rows.iter().flatten().all(|h| h.w == 1.0) {
        Ok(b::Surface::Spline { u_knots: ku, v_knots: kv, points: rows.iter().map(|r| r.iter().map(|h| [h.x, h.y, h.z]).collect()).collect() })
    } else {
        Ok(b::Surface::RationalSpline {
            u_knots: ku,
            v_knots: kv,
            homogeneous: rows.iter().map(|r| r.iter().map(|h| [h.x, h.y, h.z, h.w]).collect()).collect(),
        })
    }
}

/// The one-face sheet on `surface` bounded by the loop `chain` of `record`'s own edges, run
/// counter-clockwise about the surface's natural normal (`ccw`: the chain already does), each
/// edge's pcurve the exact projection where the surface is a height field. Its history is over
/// `[body]`: the face is new, each edge the body's own.
fn patch_body(
    body: &Body,
    record: &b::Brep,
    chain: &[(usize, bool)],
    surface: b::Surface,
    ccw: bool,
    projection: Option<&Projection>,
    tol: f64,
) -> Result<Body> {
    let fail = |m: &str| KernelError::Failed(format!("patch: {m}"));
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut vertex_of: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut edges = Vec::with_capacity(chain.len());
    let mut uses = Vec::with_capacity(chain.len());
    for (k, &(e, forward)) in chain.iter().enumerate() {
        let edge = record.edges.get(e).ok_or_else(|| fail("edge reference"))?;
        let mut ends = [0u32; 2];
        for (end, v) in ends.iter_mut().zip(edge.vertices) {
            let next = u32::try_from(vertices.len()).map_err(|_| fail("vertex budget"))?;
            *end = *vertex_of.entry(v).or_insert(next);
            if *end == next {
                vertices.push(record.vertices.get(v as usize).ok_or_else(|| fail("vertex reference"))?.point);
            }
        }
        let given = match projection {
            Some(p) => Some(p.pcurve(&edge.curve, edge.interval).ok_or_else(|| {
                KernelError::Invalid(
                    "not supported yet: a fitted patch on a circular, elliptic or traced edge (it has no exact pcurve on the fit)".into(),
                )
            })?),
            None => None,
        };
        edges.push(PartEdge { curve: edge.curve.clone(), interval: edge.interval, vertices: ends });
        uses.push(PartUse { edge: k as u32, forward, given });
    }
    if !ccw {
        uses = uses.into_iter().rev().map(|u| PartUse { forward: !u.forward, ..u }).collect();
    }
    let made = assemble(Parts { vertices, edges, faces: vec![PartFace { surface, forward: true, loops: vec![uses] }], tolerance: tol }, false)?;
    let smlib = solidcraft_smlib::Brep::from_record(&made.brep, true).map_err(|e| KernelError::Failed(format!("patch: SMLib: {e}")))?;
    let (patch, order) = crate::native::body_from_smlib_ordered(&smlib)?;
    // The patch's edges in its own order, each from the loop edge it is.
    let Some(order) = order else { return Ok(patch) };
    let mut origins: Vec<Option<usize>> = vec![None; patch.edge_count()];
    for (k, c) in made.edges.iter().enumerate() {
        let (Some(c), Some(&(e, _))) = (c, chain.get(k)) else { continue };
        let j = if order.edges.is_empty() { *c } else { *order.edges.get(*c).ok_or_else(|| fail("edge order"))? };
        *origins.get_mut(j).ok_or_else(|| fail("edge order"))? = Some(e);
    }
    let Some(origins) = origins.into_iter().collect::<Option<Vec<usize>>>() else { return Ok(patch) };
    let history = History {
        operands: 1,
        faces: vec![FaceProv { origins: Vec::new(), change: Change::Generated }],
        edges: origins.into_iter().map(|edge| EdgeProv { faces: vec![0], origins: vec![EdgeOrigin::Edge { operand: 0, edge }] }).collect(),
    };
    patch.with_history(history, &[body])
}

/// How far a loop's edges may be from a plane for its patch to be that plane: `10⁻⁷` of the
/// body's size, at least the linear tolerance (`10⁻⁶` mm) — the body's own "same point"
/// tolerance, since a planar face is exact and other operations treat it as containing its
/// edges to that tolerance. A fitted face gets [`face_tolerance`], ten times looser.
pub(crate) fn planarity_tolerance(body: &Body) -> f64 {
    (body.size() * 1e-7).max(solidcraft_geom::LINEAR_TOL)
}

/// How far the fill's surface may be from the loop's edges: `10⁻⁶` of the body's size, at least
/// `10⁻⁵` mm — ten times [`planarity_tolerance`]. Ten times the linear tolerance because a
/// fitted surface approaches curved edges like a cubic spline, so each further factor of ten
/// costs about twice the spans each way, and imported edges may already lie `10⁻⁵` mm or more off
/// their vertices. Later operations must take the face to contain its edges within this
/// tolerance, not the linear one: the face's edges are the body's exact edges, the surface
/// between them is the fit (the patch record gives its edges this tolerance).
pub(crate) fn face_tolerance(body: &Body) -> f64 {
    (body.size() * 1e-6).max(1e-5)
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::brep::{self as b, eval::CurveEval, eval::SurfaceEval};
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

    use crate::body::Body;
    use crate::boolean_record::tests::{Reach, checked};
    use crate::history::{Change, EdgeOrigin};
    use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
    use crate::{BoolOp, boolean, copy_faces, cylinder, extrude, measure, patch_edges, patch_edges_crowned, stitch};

    fn v3(p: [f64; 3]) -> Vec3 {
        Vec3::new(p[0], p[1], p[2])
    }

    /// The cutting cylinder: axis along y at x = 0, z = 50, radius 40, so the top of a prism
    /// within 20 of the axis and 20 high is cut away everywhere (to between 10 and 15.4 mm).
    fn cut_height(x: f64) -> f64 {
        50.0 - (1600.0 - x * x).sqrt()
    }

    /// A prism on the polygon `corners`, 20 high, its top cut by the cylinder; the cut solid, the
    /// open cup without its top, and a point on each rim edge (the edges' midpoints in plan).
    /// `inside` is a point inside the polygon.
    fn cup(corners: &[Vec2], inside: Vec2) -> (Body, Body, Vec<Vec3>) {
        let prism = extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(corners), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let solid = boolean(&prism, &tool, BoolOp::Cut).unwrap().unwrap();
        let n = corners.len();
        let mut keep = vec![inside.extend(0.0)];
        let mut rim = Vec::new();
        for k in 0..n {
            let m = corners[k].lerp(corners[(k + 1) % n], 0.5);
            keep.push(m.extend(5.0));
            rim.push(m.extend(cut_height(m.x)));
        }
        let open = copy_faces(&solid, &keep).unwrap();
        assert!(open.is_surface());
        (solid, open, rim)
    }

    fn regular(n: usize, phase: f64) -> Vec<Vec2> {
        (0..n).map(|k| Vec2::from_angle(phase + k as f64 * std::f64::consts::TAU / n as f64) * 20.0).collect()
    }

    /// The body's record.
    fn record(body: &Body) -> b::Brep {
        crate::native::brep_record(body).unwrap()
    }

    /// The surface of the body's first face.
    fn first_surface(body: &Body) -> b::Surface {
        record(body).faces[0].surface.clone()
    }

    /// The distance from `p` to the surface (its nearest point).
    fn to_surface(s: &b::Surface, p: Vec3) -> f64 {
        let e = SurfaceEval::new(s).unwrap();
        let uv = e.project(p, None, f64::INFINITY).expect("a nearest point");
        e.point(uv).unwrap().dist(p)
    }

    /// The farthest point along the first face's edges from its surface.
    fn edges_off_surface(body: &Body) -> f64 {
        let r = record(body);
        let face = &r.faces[0];
        let mut worst = 0.0f64;
        for l in &face.loops {
            let b::Loop::Edges { edges } = l else { continue };
            for u in edges {
                let e = &r.edges[u.edge as usize];
                let c = CurveEval::new(&e.curve).unwrap();
                let [t0, t1] = e.interval;
                for k in 0..=24 {
                    worst = worst.max(to_surface(&face.surface, c.point(t0 + (t1 - t0) * k as f64 / 24.0).unwrap()));
                }
            }
        }
        worst
    }

    /// Whether every edge of `made` is one of `of`'s own (the same curve record on the same
    /// interval).
    fn own_edges(made: &Body, of: &Body) -> bool {
        let own = record(of).edges;
        record(made).edges.iter().all(|e| own.iter().any(|o| o.curve == e.curve && o.interval == e.interval))
    }

    /// The fill's mesh: every normal finite, and no triangle folded over (all face the same way
    /// as the mesh's total area vector, by a clear margin).
    fn mesh_is_clean(fill: &Body) -> f64 {
        let m = fill.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        assert!(!m.triangles.is_empty());
        assert!(m.normals.iter().all(|n| n.is_finite() && n.len() > 0.5), "a mesh normal is not finite");
        let tri = |t: &[u32; 3]| m.tri(t).unwrap();
        let total = m.triangles.iter().map(|t| {
            let [a, b, c] = tri(t);
            (b - a).cross(c - a)
        });
        let total = total.fold(Vec3::ZERO, |s, v| s + v);
        let n = total.normalized().unwrap();
        for t in &m.triangles {
            let [a, b, c] = tri(t);
            let v = (b - a).cross(c - a);
            if v.len() > 1e-12 {
                assert!(v.dot(n) > 0.0, "a folded triangle");
            }
        }
        measure(fill).unwrap().area
    }

    /// Fill the rim, check the face, stitch it back: (the fill, the closed solid's volume).
    fn fill_rim(open: &Body, rim: &[Vec3]) -> (Body, f64) {
        let fill = patch_edges(open, rim).unwrap();
        assert!(fill.is_surface());
        assert_eq!(fill.face_count(), 1, "one face");
        assert!(!matches!(first_surface(&fill), b::Surface::Plane { .. }));
        // Its edges are the open body's own, and nothing else.
        assert!(own_edges(&fill, open), "an edge that is not the body's");
        let h = checked(&fill, &[open], 0.05, Reach { made: 0.0, moved: 0.0 }, &|_| true);
        assert!(h.faces.iter().all(|f| f.change == Change::Generated && f.origins.is_empty()));
        assert!(h.edges.iter().all(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Edge { operand: 0, .. }])));
        let off = edges_off_surface(&fill);
        let tol = super::face_tolerance(open);
        assert!(off <= tol, "an edge is {off} off the face (tolerance {tol})");
        mesh_is_clean(&fill);
        let closed = stitch(&[open, &fill], 0.0).unwrap();
        assert!(!closed.is_surface(), "the fill closes the cup");
        (fill, measure(&closed).unwrap().volume)
    }

    /// The fill's height over the point `(x, y)`: Newton's method on the surface's `x`, `y` from
    /// the middle of its face's domain (or the last answer).
    fn height_over(body: &Body, x: f64, y: f64, start: &mut Option<(f64, f64)>) -> Option<f64> {
        let r = record(body);
        let face = &r.faces[0];
        let s = SurfaceEval::new(&face.surface).ok()?;
        let [[u0, u1], [v0, v1]] = face.domain;
        let (mut u, mut v) = start.unwrap_or(((u0 + u1) / 2.0, (v0 + v1) / 2.0));
        for _ in 0..30 {
            let (q, a, b) = s.derivs(u, v)?;
            let (rx, ry) = (q.x - x, q.y - y);
            let det = a.x * b.y - a.y * b.x;
            if det.abs() < 1e-300 {
                return None;
            }
            u = (u - (b.y * rx - b.x * ry) / det).clamp(u0, u1);
            v = (v - (a.x * ry - a.y * rx) / det).clamp(v0, v1);
        }
        let q = s.point([u, v])?;
        *start = Some((u, v));
        ((q.x - x).hypot(q.y - y) < 1e-9).then_some(q.z)
    }

    /// The volume the cut prism gains with the fill for its top: the fill's height above the
    /// cylinder integrated over the polygon (midpoint rule on a 300 × 300 grid; the integrand
    /// vanishes on the rim, so cells across it add little error).
    fn gained(fill: &Body, corners: &[Vec2]) -> f64 {
        let polygon = solidcraft_geom::Ring::from(corners.to_vec());
        let solidcraft_geom::Aabb2 { min: lo, max: hi } = polygon.bounds();
        let n = 300;
        let (dx, dy) = ((hi.x - lo.x) / n as f64, (hi.y - lo.y) / n as f64);
        let inside = |x: f64, y: f64| polygon.contains(Vec2::new(x, y));
        let mut sum = 0.0;
        let mut start = None;
        for i in 0..n {
            let x = lo.x + (i as f64 + 0.5) * dx;
            for j in 0..n {
                let y = lo.y + (j as f64 + 0.5) * dy;
                if inside(x, y) {
                    let h = height_over(fill, x, y, &mut start).expect("the fill covers the polygon");
                    sum += (h - cut_height(x)) * dx * dy;
                }
            }
        }
        sum
    }

    /// The closed solid's volume against the cut prism's plus what the fill adds over the
    /// cylinder (integrated independently, [`gained`]); they agree to the integration's error
    /// (0.1% of what the fill adds) plus the error of measuring both solids on meshes
    /// (`5·10⁻⁵` of the volume).
    fn check_volume(name: &str, solid: &Body, fill: &Body, corners: &[Vec2], got: f64) {
        let prism = measure(solid).unwrap().volume;
        let extra = gained(fill, corners);
        let want = prism + extra;
        let bound = 1e-3 * extra.abs() + 5e-5 * want;
        eprintln!("{name}: closed volume {got:.4}, cut prism {prism:.4} + fill above the cylinder {extra:.4} = {want:.4} (bound ±{bound:.4})");
        assert!((got - want).abs() <= bound, "{name}: {got} vs {want}");
    }

    /// An open sheet of walls hanging from the closed loop `top` (each a curve record from one
    /// corner to the next, `corners[k]` to `corners[k + 1]`), each wall swept straight along
    /// `drop`: a plane under a line, a ruled B-spline under a spline (its pcurves its exact iso
    /// lines and rulings).
    fn walls(corners: &[Vec3], top: &[b::Curve], drop: Vec3) -> Body {
        let n = corners.len();
        let arr = |p: Vec3| [p.x, p.y, p.z];
        // Vertices: the top corners, then the bottom ones.
        let vertices: Vec<[f64; 3]> = corners.iter().map(|p| arr(*p)).chain(corners.iter().map(|p| arr(*p + drop))).collect();
        let moved = |c: &b::Curve| match c {
            b::Curve::Line { start, end } => b::Curve::Line { start: arr(v3(*start) + drop), end: arr(v3(*end) + drop) },
            b::Curve::Spline { knots, points } => {
                b::Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| arr(v3(*p) + drop)).collect() }
            }
            _ => panic!("lines and splines"),
        };
        let interval = |c: &b::Curve| match c {
            b::Curve::Spline { knots, points } => [knots[knots.len() - points.len() - 1], knots[points.len()]],
            _ => [0.0, 1.0],
        };
        let mut edges = Vec::new();
        for k in 0..n {
            let c = &top[k];
            edges.push(PartEdge { curve: c.clone(), interval: interval(c), vertices: [k as u32, ((k + 1) % n) as u32] });
            edges.push(PartEdge { curve: moved(c), interval: interval(c), vertices: [(n + k) as u32, (n + (k + 1) % n) as u32] });
            edges.push(PartEdge {
                curve: b::Curve::Line { start: arr(corners[k]), end: arr(corners[k] + drop) },
                interval: [0.0, 1.0],
                vertices: [k as u32, (n + k) as u32],
            });
        }
        let (top_edge, bottom_edge, down) = (|k: usize| 3 * k as u32, |k: usize| 3 * k as u32 + 1, |k: usize| 3 * (k % n) as u32 + 2);
        let mut faces = Vec::new();
        for k in 0..n {
            let (surface, given) = match &top[k] {
                b::Curve::Spline { knots, points } => {
                    let rows: Vec<Vec<[f64; 3]>> = points.iter().map(|p| vec![*p, arr(v3(*p) + drop)]).collect();
                    let [a, z] = interval(&top[k]);
                    let degree = knots.len() - points.len() - 1;
                    let greville = |v: f64| -> b::Curve2 {
                        let pts = (0..points.len()).map(|i| [(1..=degree).map(|r| knots[i + r]).sum::<f64>() / degree as f64, v]).collect();
                        b::Curve2::Spline { knots: knots.clone(), points: pts }
                    };
                    let ruling = |u: f64| b::Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[u, 0.0], [u, 1.0]] };
                    let surface = b::Surface::Spline { u_knots: knots.clone(), v_knots: vec![0.0, 0.0, 1.0, 1.0], points: rows };
                    (surface, Some([greville(0.0), ruling(z), greville(1.0), ruling(a)]))
                }
                _ => {
                    let (p, q) = (corners[k], corners[(k + 1) % n]);
                    let u = (q - p).normalized().unwrap();
                    let w = (drop - u * drop.dot(u)).normalized().unwrap();
                    (b::Surface::Plane { origin: arr(p), u_axis: arr(u), v_axis: arr(w) }, None)
                }
            };
            // Round the wall: along the top, down its far side, back along the bottom, up.
            let mut uses = vec![
                PartUse { edge: top_edge(k), forward: true, given: None },
                PartUse { edge: down(k + 1), forward: true, given: None },
                PartUse { edge: bottom_edge(k), forward: false, given: None },
                PartUse { edge: down(k), forward: false, given: None },
            ];
            if let Some(given) = given {
                for (u, g) in uses.iter_mut().zip(given) {
                    u.given = Some(g);
                }
            }
            faces.push(PartFace { surface, forward: true, loops: vec![uses] });
        }
        let made = assemble(Parts { vertices, edges, faces, tolerance: 1e-6 }, false).unwrap();
        crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&made.brep, true).unwrap()).unwrap()
    }

    #[test]
    fn regular_rims_of_three_to_eight_sides_fill_with_one_face_and_close_the_cup() {
        for n in 3..=8 {
            let corners = regular(n, 0.3);
            let (solid, open, rim) = cup(&corners, Vec2::ZERO);
            let (fill, got) = fill_rim(&open, &rim);
            assert_eq!(fill.edge_count(), n, "the rim's own edges");
            check_volume(&format!("{n} sides"), &solid, &fill, &corners, got);
        }
    }

    #[test]
    fn concave_rims_fill_with_one_face() {
        let v = |x: f64, y: f64| Vec2::new(x, y);
        let shapes = [
            ("dart", vec![v(-10.0, 0.0), v(15.0, -12.0), v(3.0, 0.0), v(15.0, 12.0)], v(0.0, 0.0)),
            ("arrow", vec![v(-12.0, 0.0), v(0.0, -10.0), v(0.0, -4.0), v(14.0, 0.0), v(0.0, 4.0)], v(0.0, 0.0)),
            (
                "plus",
                vec![
                    v(-4.0, -14.0),
                    v(4.0, -14.0),
                    v(4.0, -4.0),
                    v(14.0, -4.0),
                    v(14.0, 4.0),
                    v(4.0, 4.0),
                    v(4.0, 14.0),
                    v(-4.0, 14.0),
                    v(-4.0, 4.0),
                    v(-14.0, 4.0),
                    v(-14.0, -4.0),
                    v(-4.0, -4.0),
                ],
                v(0.0, 0.0),
            ),
        ];
        for (name, corners, inside) in shapes {
            let (solid, open, rim) = cup(&corners, inside);
            let (fill, got) = fill_rim(&open, &rim);
            assert_eq!(fill.edge_count(), corners.len(), "{name}: the rim's own edges");
            check_volume(name, &solid, &fill, &corners, got);
        }
    }

    #[test]
    fn a_flat_corner_between_two_edges_is_kept() {
        // A rim of five straight edges, two of them in line (a 180° corner between them), the
        // four true corners at different heights; walls hang below it.
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let v = [p(-14.0, -14.0, 12.0), p(0.0, -14.0, 12.0), p(14.0, -14.0, 12.0), p(14.0, 14.0, 15.0), p(-14.0, 14.0, 10.0)];
        let arr = |q: Vec3| [q.x, q.y, q.z];
        let top: Vec<b::Curve> = (0..5).map(|k| b::Curve::Line { start: arr(v[k]), end: arr(v[(k + 1) % 5]) }).collect();
        let body = walls(&v, &top, Vec3::new(0.0, 0.0, -10.0));
        let rim: Vec<Vec3> = (0..5).map(|k| v[k].lerp(v[(k + 1) % 5], 0.5)).collect();
        let fill = patch_edges(&body, &rim).unwrap();
        assert_eq!((fill.face_count(), fill.edge_count()), (1, 5), "one face on all five edges");
        assert!(edges_off_surface(&fill) <= super::face_tolerance(&body));
        let area = mesh_is_clean(&fill);
        eprintln!("flat corner: one face of area {area}");
        assert!(area > 28.0 * 28.0 * 0.99 && area < 28.0 * 28.0 * 1.05, "{area}");
    }

    #[test]
    fn the_fill_does_not_depend_on_pick_order_or_where_the_body_is() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        let (a, _) = fill_rim(&open, &rim);
        let mut shuffled = rim.clone();
        shuffled.rotate_left(3);
        shuffled.swap(0, 1);
        let (b, _) = fill_rim(&open, &shuffled);
        let (ra, rb) = (record(&a), record(&b));
        let (sa, sb) = (SurfaceEval::new(&ra.faces[0].surface).unwrap(), SurfaceEval::new(&rb.faces[0].surface).unwrap());
        let [[u0, u1], [v0, v1]] = ra.faces[0].domain;
        let mut gap = 0.0f64;
        for (x, y) in [(0.2, 0.3), (0.5, 0.5), (0.7, 0.9)] {
            let uv = [u0 + (u1 - u0) * x, v0 + (v1 - v0) * y];
            gap = gap.max(sa.point(uv).unwrap().dist(sb.point(uv).unwrap()));
        }
        assert!(gap < 1e-7, "pick order moved the surface by {gap}");
        let (area_a, area_b) = (mesh_is_clean(&a), mesh_is_clean(&b));
        assert!((area_a - area_b).abs() < area_a * 1e-9);
        // Moved and turned: the same area.
        let moved = crate::transform(&open, Vec3::new(130.0, -70.0, 40.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.7).unwrap();
        let ax = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap();
        let turn = |p: Vec3| {
            // Rodrigues' rotation by 0.7 about `ax`, then the shift.
            let (c, s) = (0.7f64.cos(), 0.7f64.sin());
            p * c + ax.cross(p) * s + ax * (ax.dot(p) * (1.0 - c)) + Vec3::new(130.0, -70.0, 40.0)
        };
        let moved_rim: Vec<Vec3> = rim.iter().map(|p| turn(*p)).collect();
        let (c, _) = fill_rim(&moved, &moved_rim);
        let area_c = mesh_is_clean(&c);
        eprintln!("pick order: surfaces within {gap:e}; areas {area_a} / {area_b} / moved {area_c}");
        assert!((area_a - area_c).abs() < area_a * 1e-6, "{area_a} vs moved {area_c}");
    }

    #[test]
    fn a_saddle_rim_of_two_intersection_curves_fills() {
        // A round post with its top cut by the sideways cylinder: the rim is two intersection
        // curves; the fill is one face on them.
        let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let solid = boolean(&post, &tool, BoolOp::Cut).unwrap().unwrap();
        let sides: Vec<Vec3> = (0..8).map(|k| Vec2::from_angle(k as f64 * 0.8).extend(0.0) * 10.0 + Vec3::new(0.0, 0.0, 5.0)).collect();
        let open = copy_faces(&solid, &[&[Vec3::ZERO][..], &sides].concat()).unwrap();
        let rim: Vec<Vec3> = [-0.98, 2.18]
            .map(|a: f64| {
                let (x, y) = (10.0 * a.cos(), 10.0 * a.sin());
                Vec3::new(x, y, cut_height(x))
            })
            .to_vec();
        let (fill, got) = fill_rim(&open, &rim);
        let circle: Vec<Vec2> = (0..720).map(|k| Vec2::from_angle(k as f64 * std::f64::consts::TAU / 720.0) * 10.0).collect();
        check_volume("saddle rim", &solid, &fill, &circle, got);
    }

    #[test]
    fn imported_edges_whose_vertices_are_off_their_curves_fill() {
        // The cup's vertices moved 2·10⁻⁵ mm off their curves, as a file might give them.
        let corners = regular(5, 0.3);
        let (solid, open, rim) = cup(&corners, Vec2::ZERO);
        let mut moved = record(&open);
        for v in &mut moved.vertices {
            v.point[0] += 2e-5;
            v.tolerance = v.tolerance.max(1e-4);
        }
        let imported = crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&moved, true).unwrap()).unwrap();
        let (fill, got) = fill_rim(&imported, &rim);
        check_volume("imported", &solid, &fill, &corners, got);
    }

    #[test]
    fn a_spline_edge_bulging_out_of_plane_is_never_given_a_flat_face() {
        // A flat rectangle of edges but for one cubic edge with a control point raised 1.5 mm: no
        // plane can be certified. A one-span bump is followed by the fitted face; a narrow one
        // (64 spans, about 1 mm wide, its third derivative jumping at every knot) may be beyond
        // a 32 × 32 grid, and then it is refused.
        for (spans, raised, must_fill) in [(1, 1, true), (64, 30, false)] {
            let count = spans + 3;
            let points: Vec<[f64; 3]> =
                (0..count).map(|i| [20.0 * i as f64 / (count - 1) as f64, 0.0, if i == raised { 1.5 } else { 0.0 }]).collect();
            // Uniform knots over [0, 1], clamped.
            let mut knots = vec![0.0; 4];
            knots.extend((1..spans).map(|i| i as f64 / spans as f64));
            knots.extend([1.0; 4]);
            let spline = b::Curve::Spline { knots, points };
            let c = CurveEval::new(&spline).unwrap();
            let corners: Vec<Vec3> = [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)].iter().map(|&(x, y)| Vec3::new(x, y, 0.0)).collect();
            let line = |a: Vec3, z: Vec3| b::Curve::Line { start: [a.x, a.y, a.z], end: [z.x, z.y, z.z] };
            let top = vec![spline.clone(), line(corners[1], corners[2]), line(corners[2], corners[3]), line(corners[3], corners[0])];
            let body = walls(&corners, &top, Vec3::new(0.0, 0.0, -5.0));
            let peak = c.point((raised as f64 + 1.0) / (count + 1) as f64).unwrap();
            let rim = [c.point(0.5).unwrap(), Vec3::new(20.0, 5.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 5.0, 0.0)];
            match patch_edges(&body, &rim) {
                Ok(fill) => {
                    let surface = first_surface(&fill);
                    assert!(!matches!(surface, b::Surface::Plane { .. }), "a plane would miss the bulge");
                    let (off, tol) = (edges_off_surface(&fill), super::face_tolerance(&body));
                    eprintln!("{spans}-span bump: filled, edges within {off:e} of the face");
                    assert!(off <= tol && to_surface(&surface, peak) <= tol);
                    mesh_is_clean(&fill);
                }
                Err(e) => {
                    eprintln!("{spans}-span bump: {e}");
                    assert!(!must_fill && e.to_string().contains("not supported yet"), "{e}");
                }
            }
        }
    }

    /// A point on each edge that bounds one face of `body` (its free edges).
    fn free_edge_points(body: &Body) -> Vec<Vec3> {
        let r = record(body);
        r.edges
            .iter()
            .filter(|e| e.uses.len() == 1)
            .map(|e| {
                let [t0, t1] = e.interval;
                CurveEval::new(&e.curve).unwrap().point(0.5 * (t0 + t1)).unwrap()
            })
            .collect()
    }

    #[test]
    fn a_loop_that_does_not_bound_a_height_field_fills_with_one_face() {
        // Half a round post's side: two straight edges up and two half circles, whose plane
        // stands on the post's axis; the half circles run along its normal at their ends, so no
        // height field over it bounds them (the height-field fill refuses it). The general fill
        // (#759) fills it with one face on its four edges.
        let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let halves = crate::split_by_plane(&post, &Plane::from_normal(Vec3::ZERO, Vec3::X).unwrap()).unwrap();
        let half = halves.into_iter().find(|b| measure(b).unwrap().bbox.min.x > -1e-6).unwrap();
        let at: Vec<Vec3> =
            (-4..=4).map(|k| Vec2::from_angle(k as f64 * 0.35).extend(10.0) * 1.0).map(|p| Vec3::new(p.x * 10.0, p.y * 10.0, 10.0)).collect();
        let side = copy_faces(&half, &at).unwrap();
        let rim = free_edge_points(&side);
        let fill = patch_edges(&side, &rim).unwrap();
        assert_eq!(fill.face_count(), 1, "one face");
        let (off, tol) = (edges_off_surface(&fill), super::face_tolerance(&side));
        assert!(off <= tol, "an edge is {off} off the face (tolerance {tol})");
        // Its area is at least its shadow on the plane x = 0 (the 20 × 20 square the loop bounds
        // there) and, a least-bending fill, no more than the half side it spans.
        let area = measure(&fill).unwrap().area;
        eprintln!("half a post's side: one face of area {area}, its edges within {off:e}");
        assert!(area > 400.0 && area < std::f64::consts::PI * 10.0 * 20.0, "{area}");
    }

    #[test]
    fn planar_loops_of_intersection_curves_are_one_planar_face() {
        // A round post cut by planes tilted every way, gently to steeply: each rim is an ellipse
        // of intersection curves (split where the post's side has seams) lying on the cutting
        // plane, so the patch is one planar face, and the post closes again.
        for tilt in [Vec3::new(0.0, -0.4, 1.0), Vec3::new(0.7, -0.9, 1.0), Vec3::new(1.3, 0.2, 1.0)] {
            let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 30.0).unwrap();
            let n = tilt.normalized().unwrap();
            let plane = Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), n).unwrap();
            let pieces = crate::split_by_plane(&post, &plane).unwrap();
            let solid = pieces.into_iter().find(|b| measure(b).unwrap().bbox.min.z < 0.5).unwrap();
            let height = |x: f64, y: f64| 15.0 - (tilt.x * x + tilt.y * y) / tilt.z;
            let sides: Vec<Vec3> = (0..8).map(|k| Vec2::from_angle(k as f64 * 0.8).extend(0.0) * 10.0 + Vec3::new(0.0, 0.0, 1.0)).collect();
            let open = copy_faces(&solid, &[&[Vec3::ZERO][..], &sides].concat()).unwrap();
            let rim: Vec<Vec3> = (0..12)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / 12.0 + 0.2;
                    let (x, y) = (10.0 * a.cos(), 10.0 * a.sin());
                    Vec3::new(x, y, height(x, y))
                })
                .collect();
            let lid = patch_edges(&open, &rim).unwrap();
            assert_eq!(lid.face_count(), 1);
            let edges = lid.edge_count();
            assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }), "tilt {tilt:?}");
            let closed = stitch(&[&open, &lid], 0.0).unwrap();
            assert!(!closed.is_surface());
            let (want, got) = (measure(&solid).unwrap().volume, measure(&closed).unwrap().volume);
            eprintln!("rim cut at {tilt:?}: {edges} intersection curves, planar face, volume {got} against {want}");
            assert!((got - want).abs() < want * 1e-6, "{got} vs {want}");
        }
    }

    #[test]
    fn planar_loops_are_still_one_planar_face() {
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let open = copy_faces(
            &b,
            &[Vec3::new(5.0, 5.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(0.0, 5.0, 5.0)],
        )
        .unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
        let lid = patch_edges(&open, &rim).unwrap();
        assert_eq!(lid.face_count(), 1);
        assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }));
        let closed = stitch(&[&open, &lid], 0.0).unwrap();
        assert!((measure(&closed).unwrap().volume - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn open_partial_or_hostile_picks_are_refused() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        assert!(patch_edges(&open, &rim[..4]).is_err(), "a loop with a missing edge");
        assert!(patch_edges(&open, &[]).is_err());
        assert!(patch_edges(&open, &[Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
        assert!(patch_edges(&open, &[Vec3::new(1e300, 0.0, 0.0)]).is_err());
        assert!(patch_edges(&open, &vec![rim[0]; 20_000]).is_err());
    }

    /// A crowned patch: the middle is fuller by `crown` times the loop's size, the loop's own
    /// edges stay exactly where they were (the face still meets them within [`face_tolerance`]),
    /// and a planar loop is the same exact plane whatever the crown.
    #[test]
    fn a_crowned_patch_fills_its_middle_and_keeps_its_edges() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        let plain = patch_edges(&open, &rim).unwrap();
        let same = patch_edges_crowned(&open, &rim, 0.0).unwrap();
        // Crown 0 is the plain patch: the same surface, to the last bit.
        assert_eq!(measure(&plain).unwrap().area, measure(&same).unwrap().area, "crown 0 is patch_edges");
        for crown in [0.25, -0.25] {
            let full = patch_edges_crowned(&open, &rim, crown).unwrap();
            assert_eq!(full.face_count(), 1, "one face");
            assert!(!matches!(first_surface(&full), b::Surface::Plane { .. }));
            assert!(own_edges(&full, &open), "an edge that is not the body's");
            // The loop's edges are within the fill's tolerance of the face, as for the plain fill.
            let off = edges_off_surface(&full);
            assert!(off <= super::face_tolerance(&open), "an edge is {off} off the crowned face");
            mesh_is_clean(&full);
            // The middle of the loop's box moved by crown × its diagonal (the bump is 1 there).
            let (ra, rb) = (record(&plain), record(&full));
            let (a, b) = (SurfaceEval::new(&ra.faces[0].surface).unwrap(), SurfaceEval::new(&rb.faces[0].surface).unwrap());
            let [[bu0, bu1], [bv0, bv1]] = ra.faces[0].domain;
            let (uc, vc) = ((bu0 + bu1) / 2.0, (bv0 + bv1) / 2.0);
            let size = (bu1 - bu0).hypot(bv1 - bv0);
            let bump = a.point([uc, vc]).unwrap().dist(b.point([uc, vc]).unwrap());
            let want = (crown * size).abs();
            eprintln!("crown {crown}: the middle moved {bump:.6}, wanted {want:.6}");
            assert!((bump - want).abs() < 1e-6 * size, "middle {bump} against {want}");
        }
        // A planar loop: the exact plane, crowned or not (the crown has no middle to fill).
        let boxed = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let open = copy_faces(
            &boxed,
            &[Vec3::new(5.0, 5.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(0.0, 5.0, 5.0)],
        )
        .unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
        let lid = patch_edges_crowned(&open, &rim, 1.0).unwrap();
        assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }), "a planar loop is a plane");
        // Hostile crowns are refused before anything is built.
        for crown in [f64::NAN, f64::INFINITY, 3.01, -0.91, 1e300] {
            assert!(patch_edges_crowned(&open, &rim, crown).is_err(), "crown {crown}");
        }
    }
}

#[cfg(test)]
#[path = "fill_general_tests.rs"]
mod general_tests;
